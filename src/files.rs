use http_body_util::{BodyExt, Empty, Full};
use hyper_util::client::legacy::Client;
use std::{
    sync::mpsc::Sender,
    path::PathBuf
};
use hyper::{Method, Request};
use serde::Deserialize;
use bytes::Bytes;

use super::app_state::{ReceivedFile, TailscaleEvent, TransferringFile};

#[derive(Debug, Deserialize)]
pub struct FileWaiting {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Size")]
    size: i64,
}

#[derive(Debug, Deserialize)]
pub struct IncomingFile {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "PartialPath")]
    #[allow(dead_code)]
    partial_path: Option<String>,
    #[serde(rename = "DeclaredSize")]
    size: i64,
    #[serde(rename = "Received")]
    received: Option<i64>,
    #[serde(rename = "Done")]
    done: bool,
    #[serde(rename = "FinalPath")]
    final_path: Option<String>,
}


pub async fn watch_files(event_tx: Sender<TailscaleEvent>) -> anyhow::Result<()> {
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build(super::tailscale::UnixConnector);
    let req = Request::builder()
        .uri("http://local-tailscaled.sock/localapi/v0/watch-ipn-bus")
        .header("Host", "local-tailscaled.sock")
        .body(Empty::<Bytes>::new())?;

    let res = client.request(req).await?;
    let mut body_stream = res.into_body();
    let mut buffer = String::new();

    while let Some(frame) = body_stream.frame().await {
        let frame = frame?;
        if let Some(chunk) = frame.data_ref() {
            let text = String::from_utf8_lossy(chunk);
            buffer.push_str(&text);

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].to_string();
                buffer.drain(..=pos);
                if line.trim().is_empty() {
                    continue;
                }

                log::warn!("RAW IPN BUS MESSAGE: {line}");

                if let Ok(event) = serde_json::from_str::<super::tailscale::IpnBusNotification>(&line) {
                    // Handle incoming files
                    if let Some(incoming) = event.incoming_files {
                        for file in incoming {
                            if file.done {
                                if let Some(path) = file.final_path {
                                    let _ = event_tx.send(TailscaleEvent::FileReceived(
                                        ReceivedFile {
                                            name: file.name.clone(),
                                            path: PathBuf::from(&path),
                                            size: file.size as u64,
                                            from_peer: "Unknown".to_string(),
                                            received_at: std::time::Instant::now(),
                                            saved: false,
                                        },
                                    ));
                                }
                            } else {
                                // File is still transferring
                                let _ = event_tx.send(TailscaleEvent::FileTransferring(
                                    TransferringFile {
                                        name: file.name.clone(),
                                        size: file.size as u64,
                                        transferred: file.received.unwrap_or(0) as u64,
                                        done: false,
                                    },
                                ));
                            }
                        }
                    }

                    // Handle files waiting
                    if let Some(map) = event.files_waiting {
                        for (sender_id, files) in map {
                            for file in files {
                                let _ = event_tx.send(TailscaleEvent::FileReceived(ReceivedFile {
                                    name: file.name.clone(),
                                    path: PathBuf::from("/var/lib/tailscale/files"),
                                    size: file.size as u64,
                                    from_peer: sender_id.clone(),
                                    received_at: std::time::Instant::now(),
                                    saved: false,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn send_file(
    _client: &Client<super::tailscale::UnixConnector, Empty<Bytes>>,
    peer_id: &str,
    file_path: &PathBuf,
) -> anyhow::Result<()> {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let file_content = tokio::fs::read(file_path).await?;

    // Create a new client that accepts Full<Bytes> body
    let client: Client<super::tailscale::UnixConnector, Full<Bytes>> =
        Client::builder(hyper_util::rt::TokioExecutor::new()).build(super::tailscale::UnixConnector);

    let content_length = file_content.len();

    let req = Request::builder()
        .method(Method::PUT)
        .uri(format!(
            "http://local-tailscaled.sock/localapi/v0/file-put/{}/{}",
            peer_id,
            urlencoding::encode(file_name)
        ))
        .header("Host", "local-tailscaled.sock")
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", content_length)
        .body(Full::new(Bytes::from(file_content)))?;

    log::info!("Sending file to {peer_id}: {file_name}");

    let res = client.request(req).await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.into_body().collect().await?.to_bytes();
        let body_text = String::from_utf8_lossy(&body);
        anyhow::bail!("File send failed with status: {} - {}", status, body_text);
    }

    Ok(())
}
