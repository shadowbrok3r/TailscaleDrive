use http_body_util::{BodyExt, Empty};
use hyper::{Request, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::Connection;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::task::{Context, Poll};
use std::future::Future;

use bytes::Bytes;

use tokio::sync::mpsc as tokio_mpsc;
use serde::Deserialize;
use super::app_state::{TailscaleCommand, TailscaleEvent, TailscalePeer};

// --- Connector Logic ---
#[derive(Clone)]
pub struct UnixConnector;

impl tower::Service<Uri> for UnixConnector {
    type Response = TokioIo<UnixStream>;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Uri) -> Self::Future {
        Box::pin(async move {
            let stream = UnixStream::connect("/var/run/tailscale/tailscaled.sock").await?;
            Ok(TokioIo::new(stream))
        })
    }
}

impl Connection for UnixConnector {
    fn connected(&self) -> hyper_util::client::legacy::connect::Connected {
        hyper_util::client::legacy::connect::Connected::new()
    }
}

// --- Tailscale API Data Structures ---

#[derive(Debug, Deserialize)]
pub struct TailscaleStatus {
    #[serde(rename = "BackendState")]
    #[allow(dead_code)]
    backend_state: String,
    #[serde(rename = "Self")]
    self_node: Option<PeerStatus>,
    #[serde(rename = "Peer")]
    peers: Option<HashMap<String, PeerStatus>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PeerStatus {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "HostName")]
    hostname: String,
    #[serde(rename = "DNSName")]
    dns_name: String,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Option<Vec<String>>,
    #[serde(rename = "Online")]
    online: bool,
    #[serde(rename = "OS")]
    os: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IpnBusNotification {
    #[serde(rename = "FilesWaiting")]
    pub files_waiting: Option<HashMap<String, Vec<super::files::FileWaiting>>>,
    #[serde(rename = "IncomingFiles")]
    pub incoming_files: Option<Vec<super::files::IncomingFile>>,
}


// --- Background Tailscale Tasks ---

pub async fn run_tailscale_backend(
    event_tx: Sender<TailscaleEvent>,
    mut command_rx: tokio_mpsc::UnboundedReceiver<TailscaleCommand>,
    app_state: super::status::AppState,
) -> anyhow::Result<()> {
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build(UnixConnector);

    // Initial status fetch
    let _ = event_tx.send(TailscaleEvent::ConnectionStatus(
        false,
        "Connecting to Tailscale...".to_string(),
    ));

    // Fetch initial peer list
    match fetch_status(&client).await {
        Ok(peers) => {
            // Update shared state for HTTP server
            {
                let mut shared_peers = app_state.peers.lock().unwrap();
                *shared_peers = peers.clone();
            }
            let _ = event_tx.send(TailscaleEvent::PeersUpdated(peers));
            let _ = event_tx.send(TailscaleEvent::ConnectionStatus(
                true,
                "Connected to Tailscale".to_string(),
            ));
        }
        Err(e) => {
            let _ = event_tx.send(TailscaleEvent::Error(format!(
                "Failed to connect: {}",
                e
            )));
        }
    }

    // Auto-configure `tailscale serve` to expose port 8080 on the tailnet
    tokio::spawn(async {
        match tokio::process::Command::new("tailscale")
            .args(["serve", "--bg", "8080"])
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if output.status.success() {
                    log::info!("Tailscale serve configured: {stdout}");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!("Tailscale serve setup: {}", stderr.trim());
                }
            }
            Err(e) => log::error!("Failed to run 'tailscale serve': {}", e),
        }
    });

    // Spawn status HTTP server (0.0.0.0:8080)
    let state_for_server = app_state.clone();
    let status_handle = tokio::spawn(async move {
        if let Err(e) = super::status::run_status_server(state_for_server).await {
            log::error!("Status server error: {:?}", e);
        }
    });

    // Spawn IPN bus watcher (must stay lean — no blocking calls in the read loop)
    let event_tx_watcher = event_tx.clone();
    let received_for_watcher = app_state.received.clone();
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = super::files::watch_files(event_tx_watcher, received_for_watcher).await {
            log::error!("File watcher error: {:?}", e);
        }
    });

    // Spawn periodic check for waiting files via the API.
    // Catches files received before the app started.
    let event_tx_files = event_tx.clone();
    let received_for_checker = app_state.received.clone();
    let files_check_handle = tokio::spawn(async move {
        let files_client = Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(UnixConnector);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Ok(waiting) = super::files::fetch_waiting_files(&files_client).await {
                for wf in waiting {
                    // Update the received state so the download server knows about these files
                    {
                        let mut state = received_for_checker.lock().unwrap();
                        if state.last_file.is_none() {
                            state.last_file = Some(wf.name.clone());
                        }
                    }

                    let _ = event_tx_files.send(TailscaleEvent::FileReceived(
                        super::app_state::ReceivedFile {
                            name: wf.name.clone(),
                            path: None,
                            size: wf.size as u64,
                            from_peer: "Unknown".to_string(),
                            received_at: std::time::Instant::now(),
                            saved: false,
                        },
                    ));
                }
            }
        }
    });

    // Spawn periodic peer refresh
    let event_tx_status = event_tx.clone();
    let client_clone = client.clone();
    let peers_shared = app_state.peers.clone();
    let refresh_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Ok(peers) = fetch_status(&client_clone).await {
                // Update shared state for HTTP server
                {
                    let mut shared = peers_shared.lock().unwrap();
                    *shared = peers.clone();
                }
                let _ = event_tx_status.send(TailscaleEvent::PeersUpdated(peers));
            }
        }
    });

    // Handle commands from UI
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            TailscaleCommand::SendFile { peer_id, file_path } => {
                let client = client.clone();
                let event_tx = event_tx.clone();
                let last_sent = app_state.last_sent.clone();
                tokio::spawn(async move {
                    let file_name = file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string();
                    let file_size = tokio::fs::metadata(&file_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);

                    // Mark as currently sending
                    {
                        let mut state = last_sent.lock().unwrap();
                        *state = Some(super::status::SentFileInfo {
                            name: file_name.clone(),
                            peer_id: peer_id.clone(),
                            size: file_size,
                            timestamp: super::status::unix_timestamp(),
                            succeeded: false,
                            sending: true,
                        });
                    }

                    let result =
                        super::files::send_file(&client, &peer_id, &file_path).await;

                    // Update with final result
                    {
                        let mut state = last_sent.lock().unwrap();
                        *state = Some(super::status::SentFileInfo {
                            name: file_name,
                            peer_id: peer_id.clone(),
                            size: file_size,
                            timestamp: super::status::unix_timestamp(),
                            succeeded: result.is_ok(),
                            sending: false,
                        });
                    }

                    if let Err(e) = result {
                        let _ = event_tx.send(TailscaleEvent::Error(format!(
                            "Failed to send file: {}",
                            e
                        )));
                    }
                });
            }
            TailscaleCommand::RefreshPeers => {
                if let Ok(peers) = fetch_status(&client).await {
                    {
                        let mut shared = app_state.peers.lock().unwrap();
                        *shared = peers.clone();
                    }
                    let _ = event_tx.send(TailscaleEvent::PeersUpdated(peers));
                }
            }
            TailscaleCommand::SaveReceivedFile { name, src_path, dest } => {
                let event_tx = event_tx.clone();
                tokio::spawn(async move {
                    let save_result = if let Some(src) = &src_path {
                        // Fast path: copy directly from FinalPath on disk
                        tokio::fs::copy(src, &dest).await
                            .map(|_| ())
                            .map_err(|e| anyhow::anyhow!("fs::copy failed: {}", e))
                    } else {
                        // Fallback: download via local API
                        match super::files::download_received_file(&name).await {
                            Ok(content) => tokio::fs::write(&dest, &content).await
                                .map_err(|e| anyhow::anyhow!("write failed: {}", e)),
                            Err(e) => Err(e),
                        }
                    };

                    match save_result {
                        Ok(()) => {
                            log::info!("Saved '{}' to {:?}", name, dest);
                            // Clean up from the Taildrop inbox
                            if let Err(e) = super::files::delete_received_file(&name).await {
                                log::warn!("Failed to clean up '{}' from inbox: {}", name, e);
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(TailscaleEvent::Error(format!(
                                "Failed to save file '{}': {}", name, e
                            )));
                        }
                    }
                });
            }
            TailscaleCommand::DeleteReceivedFile(name) => {
                let event_tx = event_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = super::files::delete_received_file(&name).await {
                        let _ = event_tx.send(TailscaleEvent::Error(format!(
                            "Failed to delete file '{}': {}", name, e
                        )));
                    }
                });
            }
        }
    }

    watcher_handle.abort();
    files_check_handle.abort();
    refresh_handle.abort();
    status_handle.abort();
    Ok(())
}

pub async fn fetch_status(
    client: &Client<UnixConnector, Empty<Bytes>>,
) -> anyhow::Result<Vec<TailscalePeer>> {
    let req = Request::builder()
        .uri("http://local-tailscaled.sock/localapi/v0/status")
        .header("Host", "local-tailscaled.sock")
        .body(Empty::<Bytes>::new())?;

    let res = client.request(req).await?;
    let body = res.into_body().collect().await?.to_bytes();
    let status: TailscaleStatus = serde_json::from_slice(&body)?;

    let mut peers = Vec::new();

    // Add self
    if let Some(self_node) = status.self_node {
        peers.push(TailscalePeer {
            id: self_node.id,
            hostname: self_node.hostname,
            dns_name: self_node.dns_name,
            ip_addresses: self_node.tailscale_ips.unwrap_or_default(),
            online: true,
            is_self: true,
            os: self_node.os.unwrap_or_default(),
            can_receive_files: true,
        });
    }

    // Add other peers
    if let Some(peer_map) = status.peers {
        for (_, peer) in peer_map {
            peers.push(TailscalePeer {
                id: peer.id,
                hostname: peer.hostname,
                dns_name: peer.dns_name,
                ip_addresses: peer.tailscale_ips.unwrap_or_default(),
                online: peer.online,
                is_self: false,
                os: peer.os.unwrap_or_default(),
                can_receive_files: true,
            });
        }
    }

    // Sort: online first, then alphabetically
    let peers_with_os = peers.iter().filter(|p| !p.os.is_empty()).cloned().collect::<Vec<_>>();

    Ok(peers_with_os)
}
