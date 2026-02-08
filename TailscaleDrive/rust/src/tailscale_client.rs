use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;

// ── Data models (match desktop server JSON) ─────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SentFileInfo {
    pub name: String,
    pub peer_id: String,
    pub size: u64,
    pub timestamp: u64,
    pub succeeded: bool,
    pub sending: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaitingFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteFile {
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: u64,
}

// ── Events / Commands ───────────────────────────────────────────────────

pub enum ClientEvent {
    StatusUpdate {
        connected: bool,
        last_sent: Option<SentFileInfo>,
        last_received_file: Option<String>,
        server_cwd: Option<String>,
    },
    FilesUpdate(Vec<WaitingFile>),
    BrowseUpdate(Vec<RemoteFile>),
    DownloadComplete { filename: String, data: Vec<u8> },
    PullComplete { filename: String, data: Vec<u8> },
    Error(String),
}

pub enum ClientCommand {
    DownloadFile(String),
    DownloadLast,
    Browse(Option<String>), // optional path
    PullFile(String),       // pull a remote file by name
    Refresh,
}

// ── Public client used by the Renderer ──────────────────────────────────

pub struct TailscaleClient {
    pub server_url: String,
    pub connected: bool,
    pub status_message: String,
    pub last_sent: Option<SentFileInfo>,
    pub last_received_file: Option<String>,
    pub waiting_files: Vec<WaitingFile>,
    pub remote_files: Vec<RemoteFile>,
    pub download_status: Option<String>,
    pub browse_status: Option<String>,
    pub server_cwd: Option<String>,
    pub save_directory: Option<String>,
    /// Full paths to files that were just saved and are ready for the iOS share sheet.
    pub pending_share_paths: Vec<String>,

    event_rx: mpsc::Receiver<ClientEvent>,
    command_tx: mpsc::Sender<ClientCommand>,
}

impl TailscaleClient {
    pub fn new(server_url: &str) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();

        let url = server_url.trim_end_matches('/').to_string();
        std::thread::spawn(move || {
            poll_loop(&url, event_tx, command_rx);
        });

        Self {
            server_url: server_url.to_string(),
            connected: false,
            status_message: "Connecting…".to_string(),
            last_sent: None,
            last_received_file: None,
            waiting_files: Vec::new(),
            remote_files: Vec::new(),
            download_status: None,
            browse_status: None,
            server_cwd: None,
            save_directory: None,
            pending_share_paths: Vec::new(),
            event_rx,
            command_tx,
        }
    }

    /// Drain the event channel from the background thread.
    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ClientEvent::StatusUpdate {
                    connected,
                    last_sent,
                    last_received_file,
                    server_cwd,
                } => {
                    self.connected = connected;
                    self.last_sent = last_sent;
                    self.last_received_file = last_received_file;
                    if server_cwd.is_some() {
                        self.server_cwd = server_cwd;
                    }
                    self.status_message = if connected {
                        "Connected to server".to_string()
                    } else {
                        "Cannot reach server".to_string()
                    };
                }
                ClientEvent::FilesUpdate(files) => {
                    self.waiting_files = files;
                }
                ClientEvent::BrowseUpdate(files) => {
                    self.browse_status =
                        Some(format!("Found {} items", files.len()));
                    self.remote_files = files;
                }
                ClientEvent::DownloadComplete { filename, data } => {
                    let size = data.len();
                    if let Some(ref dir) = self.save_directory {
                        let path = format!("{}/{}", dir, filename);
                        match std::fs::write(&path, &data) {
                            Ok(_) => {
                                self.download_status = Some(format!(
                                    "✓ Saved '{}' ({})",
                                    filename,
                                    format_size(size as u64)
                                ));
                                self.pending_share_paths.push(path);
                            }
                            Err(e) => {
                                self.download_status = Some(format!(
                                    "✗ Failed to save '{}': {}",
                                    filename, e
                                ));
                            }
                        }
                    } else {
                        self.download_status = Some(format!(
                            "✓ Downloaded '{}' ({}) — no save directory set",
                            filename,
                            format_size(size as u64)
                        ));
                    }
                }
                ClientEvent::PullComplete { filename, data } => {
                    let size = data.len();
                    if let Some(ref dir) = self.save_directory {
                        let path = format!("{}/{}", dir, filename);
                        match std::fs::write(&path, &data) {
                            Ok(_) => {
                                self.browse_status = Some(format!(
                                    "✓ Saved '{}' ({})",
                                    filename,
                                    format_size(size as u64)
                                ));
                                self.pending_share_paths.push(path);
                            }
                            Err(e) => {
                                self.browse_status = Some(format!(
                                    "✗ Failed to save '{}': {}",
                                    filename, e
                                ));
                            }
                        }
                    } else {
                        self.browse_status = Some(format!(
                            "✓ Pulled '{}' ({}) — no save directory set",
                            filename,
                            format_size(size as u64)
                        ));
                    }
                }
                ClientEvent::Error(msg) => {
                    self.download_status = Some(format!("✗ {}", msg));
                }
            }
        }
    }

    pub fn download_file(&self, name: &str) {
        let _ = self.command_tx.send(ClientCommand::DownloadFile(name.to_string()));
    }

    pub fn download_last(&self) {
        let _ = self.command_tx.send(ClientCommand::DownloadLast);
    }

    pub fn browse(&self, path: Option<String>) {
        let _ = self.command_tx.send(ClientCommand::Browse(path));
    }

    pub fn pull_file(&self, name: &str) {
        let _ = self.command_tx.send(ClientCommand::PullFile(name.to_string()));
    }

    pub fn refresh(&self) {
        let _ = self.command_tx.send(ClientCommand::Refresh);
    }
}

// ── Background polling thread ───────────────────────────────────────────

fn poll_loop(
    base_url: &str,
    event_tx: mpsc::Sender<ClientEvent>,
    command_rx: mpsc::Receiver<ClientCommand>,
) {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(8))
        .timeout_write(Duration::from_secs(8))
        .build();

    let poll_interval = Duration::from_secs(3);
    let mut last_poll = Instant::now() - poll_interval; // poll immediately on start

    loop {
        // ── Process commands (non-blocking) ──
        loop {
            match command_rx.try_recv() {
                Ok(cmd) => match cmd {
                    ClientCommand::DownloadFile(name) => {
                        match http_download_file(&agent, base_url, &name) {
                            Ok(data) => {
                                let filename = name;
                                if event_tx
                                    .send(ClientEvent::DownloadComplete { filename, data })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::DownloadLast => {
                        match http_download_last(&agent, base_url) {
                            Ok((name, data)) => {
                                if event_tx
                                    .send(ClientEvent::DownloadComplete { filename: name, data })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::Browse(path) => {
                        match http_fetch_browse(&agent, base_url, path.as_deref()) {
                            Ok(files) => {
                                if event_tx.send(ClientEvent::BrowseUpdate(files)).is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::PullFile(path) => {
                        match http_pull_remote_file(&agent, base_url, &path) {
                            Ok((filename, data)) => {
                                if event_tx
                                    .send(ClientEvent::PullComplete { filename, data })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::Refresh => {
                        last_poll = Instant::now() - poll_interval;
                    }
                },
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return, // client dropped
            }
        }

        // ── Periodic polling ──
        if last_poll.elapsed() >= poll_interval {
            last_poll = Instant::now();

            match http_fetch_status(&agent, base_url) {
                Ok((last_sent, last_received, server_cwd)) => {
                    if event_tx
                        .send(ClientEvent::StatusUpdate {
                            connected: true,
                            last_sent,
                            last_received_file: last_received,
                            server_cwd,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => {
                    if event_tx
                        .send(ClientEvent::StatusUpdate {
                            connected: false,
                            last_sent: None,
                            last_received_file: None,
                            server_cwd: None,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }

            if let Ok(files) = http_fetch_files(&agent, base_url) {
                if event_tx.send(ClientEvent::FilesUpdate(files)).is_err() {
                    return;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

// ── HTTP helpers ────────────────────────────────────────────────────────

fn http_fetch_status(
    agent: &ureq::Agent,
    base_url: &str,
) -> Result<(Option<SentFileInfo>, Option<String>, Option<String>), String> {
    let url = format!("{}/status", base_url);
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let last_sent: Option<SentFileInfo> = json
        .get("last_sent_file")
        .and_then(|v| {
            if v.is_null() {
                None
            } else {
                serde_json::from_value(v.clone()).ok()
            }
        });

    let last_received: Option<String> = json
        .get("last_received_file")
        .and_then(|v| {
            if v.is_null() {
                None
            } else {
                v.as_str().map(String::from)
            }
        });

    let server_cwd: Option<String> = json
        .get("server_cwd")
        .and_then(|v| v.as_str().map(String::from));

    Ok((last_sent, last_received, server_cwd))
}

fn http_fetch_files(agent: &ureq::Agent, base_url: &str) -> Result<Vec<WaitingFile>, String> {
    let url = format!("{}/files", base_url);
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let files: Vec<WaitingFile> = json
        .get("files")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    Ok(files)
}

fn http_fetch_browse(
    agent: &ureq::Agent,
    base_url: &str,
    path: Option<&str>,
) -> Result<Vec<RemoteFile>, String> {
    let url = format!("{}/browse", base_url);
    let mut req = agent.get(&url);
    // Use .query() for proper URL-encoding (fixes 404 with spaces / special chars)
    if let Some(p) = path {
        req = req.query("path", p);
    }
    let body = req
        .call()
        .map_err(|e| format!("browse request failed: {}", e))?
        .into_string()
        .map_err(|e| e.to_string())?;

    let files: Vec<RemoteFile> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(files)
}

fn http_download_file(
    agent: &ureq::Agent,
    base_url: &str,
    name: &str,
) -> Result<Vec<u8>, String> {
    let url = format!("{}/download/{}", base_url, name);
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    resp.into_reader()
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    Ok(data)
}

fn http_download_last(agent: &ureq::Agent, base_url: &str) -> Result<(String, Vec<u8>), String> {
    let url = format!("{}/download", base_url);
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;

    // Try to get filename from Content-Disposition header
    let name = resp
        .header("content-disposition")
        .and_then(|cd| {
            cd.split("filename=\"")
                .nth(1)
                .and_then(|s| s.strip_suffix('"'))
        })
        .unwrap_or("downloaded_file")
        .to_string();

    let mut data = Vec::new();
    resp.into_reader()
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    Ok((name, data))
}

/// GET /pull?path=<filepath> — download an arbitrary file from the server's filesystem
fn http_pull_remote_file(
    agent: &ureq::Agent,
    base_url: &str,
    path: &str,
) -> Result<(String, Vec<u8>), String> {
    let url = format!("{}/pull", base_url);
    let resp = agent
        .get(&url)
        .query("path", path)
        .call()
        .map_err(|e| format!("pull request failed: {}", e))?;

    // Try to get filename from Content-Disposition header, fall back to path basename
    let name = resp
        .header("content-disposition")
        .and_then(|cd| {
            cd.split("filename=\"")
                .nth(1)
                .and_then(|s| s.strip_suffix('"'))
        })
        .map(String::from)
        .unwrap_or_else(|| {
            path.rsplit('/')
                .next()
                .unwrap_or("file")
                .to_string()
        });

    let mut data = Vec::new();
    resp.into_reader()
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    Ok((name, data))
}

// ── Utility ─────────────────────────────────────────────────────────────

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "Unknown".to_string();
    }
    // Simple relative time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hr ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}
