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

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub hostname: String,
    pub dns_name: String,
    pub ip_addresses: Vec<String>,
    pub online: bool,
    pub os: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct SyncProject {
    pub id: String,
    pub local_path: String,
    pub remote_path: String,
    pub last_synced: u64,
    pub paused: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncChange {
    pub id: String,
    pub remote_path: String,
    pub local_path: String,
    pub new_modified: u64,
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
    PeersUpdate(Vec<PeerInfo>),
    SyncProjectsUpdate(Vec<SyncProject>),
    SyncChangesAvailable(Vec<SyncChange>),
    UploadComplete { remote_path: String },
    SyncPullComplete { project_id: String, filename: String },
    Error(String),
}

pub enum ClientCommand {
    DownloadFile(String),
    DownloadLast,
    Browse(Option<String>),
    PullFile(String),
    Refresh,
    UploadFile { local_path: String, remote_dest_path: String },
    CreateSyncProject { local_path: String, remote_path: String },
    FetchSyncProjects,
    DeleteSyncProject(String),
    AckSync { id: String, timestamp: u64 },
    CheckSyncChanges,
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
    /// Tailscale peers from the connected desktop
    pub peers: Vec<PeerInfo>,
    /// Tracked sync projects
    pub sync_projects: Vec<SyncProject>,
    /// Sync status message for UI
    pub sync_status: Option<String>,
    /// Pending notifications for sync events: (title, body)
    pub pending_sync_notifications: Vec<(String, String)>,

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
            peers: Vec::new(),
            sync_projects: Vec::new(),
            sync_status: None,
            pending_sync_notifications: Vec::new(),
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
                    let was_connected = self.connected;
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
                    // When we lose connection, mark all cached peers as offline
                    // instead of wiping the list
                    if was_connected && !connected {
                        for peer in &mut self.peers {
                            peer.online = false;
                        }
                    }
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
                ClientEvent::PeersUpdate(peers) => {
                    self.peers = peers;
                    // Cache to disk for offline access
                    if let Some(ref dir) = self.save_directory {
                        save_cached_peers(dir, &self.peers);
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
                ClientEvent::SyncProjectsUpdate(projects) => {
                    self.sync_projects = projects;
                }
                ClientEvent::SyncChangesAvailable(changes) => {
                    // Auto-pull will handle these in the poll loop
                    if !changes.is_empty() {
                        self.sync_status = Some(format!(
                            "{} file(s) updated on desktop",
                            changes.len()
                        ));
                    }
                }
                ClientEvent::UploadComplete { remote_path } => {
                    let filename = remote_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&remote_path)
                        .to_string();
                    self.sync_status = Some(format!("✓ Uploaded '{}'", filename));
                }
                ClientEvent::SyncPullComplete { project_id: _, filename } => {
                    self.sync_status = Some(format!("✓ Synced '{}'", filename));
                    self.pending_sync_notifications.push((
                        "File Synced".to_string(),
                        format!("Updated: {}", filename),
                    ));
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

    pub fn upload_file(&self, local_path: &str, remote_dest_path: &str) {
        let _ = self.command_tx.send(ClientCommand::UploadFile {
            local_path: local_path.to_string(),
            remote_dest_path: remote_dest_path.to_string(),
        });
    }

    pub fn create_sync_project(&self, local_path: &str, remote_path: &str) {
        let _ = self.command_tx.send(ClientCommand::CreateSyncProject {
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
        });
    }

    pub fn fetch_sync_projects(&self) {
        let _ = self.command_tx.send(ClientCommand::FetchSyncProjects);
    }

    pub fn delete_sync_project(&self, id: &str) {
        let _ = self.command_tx.send(ClientCommand::DeleteSyncProject(id.to_string()));
    }

    pub fn check_sync_changes(&self) {
        let _ = self.command_tx.send(ClientCommand::CheckSyncChanges);
    }
}

// ── Background polling thread ───────────────────────────────────────────

fn poll_loop(
    base_url: &str,
    event_tx: mpsc::Sender<ClientEvent>,
    command_rx: mpsc::Receiver<ClientCommand>,
) {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build();

    let agent = config.into();

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
                    ClientCommand::UploadFile { local_path, remote_dest_path } => {
                        match http_upload_file(&agent, base_url, &local_path, &remote_dest_path) {
                            Ok(()) => {
                                if event_tx
                                    .send(ClientEvent::UploadComplete { remote_path: remote_dest_path })
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
                    ClientCommand::CreateSyncProject { local_path, remote_path } => {
                        match http_create_sync_project(&agent, base_url, &local_path, &remote_path) {
                            Ok(project) => {
                                // Also save locally
                                save_local_sync_project(&project);
                                // Refresh projects list
                                if let Ok(projects) = http_fetch_sync_projects(&agent, base_url) {
                                    if event_tx.send(ClientEvent::SyncProjectsUpdate(projects)).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::FetchSyncProjects => {
                        match http_fetch_sync_projects(&agent, base_url) {
                            Ok(projects) => {
                                if event_tx.send(ClientEvent::SyncProjectsUpdate(projects)).is_err() {
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
                    ClientCommand::DeleteSyncProject(id) => {
                        match http_delete_sync_project(&agent, base_url, &id) {
                            Ok(()) => {
                                remove_local_sync_project(&id);
                                if let Ok(projects) = http_fetch_sync_projects(&agent, base_url) {
                                    if event_tx.send(ClientEvent::SyncProjectsUpdate(projects)).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    ClientCommand::AckSync { id, timestamp } => {
                        let _ = http_sync_ack(&agent, base_url, &id, timestamp);
                    }
                    ClientCommand::CheckSyncChanges => {
                        match http_sync_check(&agent, base_url) {
                            Ok(changes) => {
                                if !changes.is_empty() {
                                    if event_tx.send(ClientEvent::SyncChangesAvailable(changes)).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                if event_tx.send(ClientEvent::Error(e)).is_err() {
                                    return;
                                }
                            }
                        }
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

            if let Ok(peers) = http_fetch_peers(&agent, base_url) {
                if event_tx.send(ClientEvent::PeersUpdate(peers)).is_err() {
                    return;
                }
            }

            // ── Auto-sync: check for remote changes and pull them ──
            if let Ok(changes) = http_sync_check(&agent, base_url) {
                for change in &changes {
                    // Pull the changed file from desktop
                    if let Ok((filename, data)) = http_pull_remote_file(&agent, base_url, &change.local_path) {
                        // The change.remote_path is the iOS local path
                        // Save to that path
                        if std::fs::write(&change.remote_path, &data).is_ok() {
                            // Acknowledge the sync
                            let _ = http_sync_ack(&agent, base_url, &change.id, change.new_modified);
                            if event_tx
                                .send(ClientEvent::SyncPullComplete {
                                    project_id: change.id.clone(),
                                    filename,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            }

            // ── Auto-sync: check for local changes and push them ──
            if let Ok(projects) = http_fetch_sync_projects(&agent, base_url) {
                for project in &projects {
                    if project.paused {
                        continue;
                    }
                    // project.remote_path is the iOS local path (from desktop's perspective)
                    let ios_path = &project.remote_path;
                    if let Ok(metadata) = std::fs::metadata(ios_path) {
                        let modified = metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if modified > project.last_synced {
                            // File changed locally on iOS, push to desktop
                            if http_upload_file(&agent, base_url, ios_path, &project.local_path).is_ok() {
                                // Update last_synced
                                let _ = http_sync_ack(&agent, base_url, &project.id, modified);
                                let filename = ios_path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(ios_path)
                                    .to_string();
                                if event_tx
                                    .send(ClientEvent::SyncPullComplete {
                                        project_id: project.id.clone(),
                                        filename,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
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
        .body_mut()
        .read_to_string()
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
        .body_mut()
        .read_to_string()
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
        .body_mut()
        .read_to_string()
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
    let mut resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let data = resp.body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    
    Ok(data)
}

fn http_download_last(agent: &ureq::Agent, base_url: &str) -> Result<(String, Vec<u8>), String> {
    let url = format!("{}/download", base_url);
    let mut resp = agent.get(&url).call().map_err(|e| e.to_string())?;

    // Try to get filename from Content-Disposition header
    let name = resp
        .headers()
        .get("content-disposition")
        .and_then(|cd| cd.to_str().ok())
        .and_then(|cd| {
            cd.split("filename=\"")
                .nth(1)
                .and_then(|s| s.strip_suffix('"'))
        })
        .unwrap_or("downloaded_file")
        .to_string();

     let data =resp.body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;

    Ok((name, data))
}

fn http_fetch_peers(agent: &ureq::Agent, base_url: &str) -> Result<Vec<PeerInfo>, String> {
    let url = format!("{}/peers", base_url);
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;

    let peers: Vec<PeerInfo> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(peers)
}

/// GET /pull?path=<filepath> — download an arbitrary file from the server's filesystem
fn http_pull_remote_file(
    agent: &ureq::Agent,
    base_url: &str,
    path: &str,
) -> Result<(String, Vec<u8>), String> {
    let url = format!("{}/pull", base_url);
    let mut resp = agent
        .get(&url)
        .query("path", path)
        .call()
        .map_err(|e| format!("pull request failed: {}", e))?;

    // Try to get filename from Content-Disposition header, fall back to path basename
    let name = resp
        .headers()
        .get("content-disposition")
        .and_then(|cd| cd.to_str().ok())
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

    let data = resp.body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;

    Ok((name, data))
}

// ── Sync HTTP helpers ───────────────────────────────────────────────

fn http_upload_file(
    agent: &ureq::Agent,
    base_url: &str,
    local_path: &str,
    remote_dest_path: &str,
) -> Result<(), String> {
    let data = std::fs::read(local_path)
        .map_err(|e| format!("Failed to read '{}': {}", local_path, e))?;

    let url = format!("{}/sync/upload", base_url);
    agent
        .put(&url)
        .query("path", remote_dest_path)
        .send(&data)
        .map_err(|e| format!("upload failed: {}", e))?;

    Ok(())
}

fn http_create_sync_project(
    agent: &ureq::Agent,
    base_url: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<SyncProject, String> {
    let url = format!("{}/sync/projects", base_url);
    // Note: from the desktop's perspective, local_path is the desktop path (remote_path here)
    // and remote_path is the iOS path (local_path here)
    let body = serde_json::json!({
        "local_path": remote_path,
        "remote_path": local_path,
    });

    let mut resp = agent
        .post(&url)
        .header("Content-Type", "application/json")
        .send(&body.to_string())
        .map_err(|e| format!("create sync project failed: {}", e))?;

    let text = resp.body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let project: SyncProject = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(project)
}

fn http_fetch_sync_projects(
    agent: &ureq::Agent,
    base_url: &str,
) -> Result<Vec<SyncProject>, String> {
    let url = format!("{}/sync/projects", base_url);
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;

    let projects: Vec<SyncProject> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(projects)
}

fn http_delete_sync_project(
    agent: &ureq::Agent,
    base_url: &str,
    id: &str,
) -> Result<(), String> {
    let url = format!("{}/sync/projects/{}", base_url, id);
    agent
        .delete(&url)
        .call()
        .map_err(|e| format!("delete sync project failed: {}", e))?;
    Ok(())
}

fn http_sync_check(
    agent: &ureq::Agent,
    base_url: &str,
) -> Result<Vec<SyncChange>, String> {
    let url = format!("{}/sync/check", base_url);
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;

    let changes: Vec<SyncChange> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(changes)
}

fn http_sync_ack(
    agent: &ureq::Agent,
    base_url: &str,
    id: &str,
    timestamp: u64,
) -> Result<(), String> {
    let url = format!("{}/sync/ack", base_url);
    let body = serde_json::json!({ "id": id, "timestamp": timestamp });
    agent
        .post(&url)
        .header("Content-Type", "application/json")
        .send(&body.to_string())
        .map_err(|e| format!("sync ack failed: {}", e))?;
    Ok(())
}

// ── Peer caching (iOS side) ─────────────────────────────────────────

fn cached_peers_path(save_dir: &str) -> String {
    // Go up from "Downloads" to "Documents" for the cache file
    if let Some(parent) = std::path::Path::new(save_dir).parent() {
        format!("{}/cached_peers.json", parent.to_string_lossy())
    } else {
        format!("{}/cached_peers.json", save_dir)
    }
}

pub fn load_cached_peers(save_dir: &str) -> Vec<PeerInfo> {
    let path = cached_peers_path(save_dir);
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_cached_peers(save_dir: &str, peers: &[PeerInfo]) {
    let path = cached_peers_path(save_dir);
    if let Ok(data) = serde_json::to_string_pretty(peers) {
        let _ = std::fs::write(&path, data);
    }
}

// ── Local sync project persistence (iOS side) ──────────────────────

fn local_sync_projects_path() -> Option<String> {
    // Use the app's Documents directory
    // This will be set via save_directory, but for persistence we go up one level
    None // Will be computed from save_directory at runtime
}

fn load_local_sync_projects_from(dir: &str) -> Vec<SyncProject> {
    let path = format!("{}/sync_projects.json", dir);
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_local_sync_projects_to(dir: &str, projects: &[SyncProject]) {
    let path = format!("{}/sync_projects.json", dir);
    if let Ok(data) = serde_json::to_string_pretty(projects) {
        let _ = std::fs::write(&path, data);
    }
}

fn save_local_sync_project(project: &SyncProject) {
    // We'll save to a well-known location; the renderer will call with save_directory
    // For now, store in a static-like approach using /tmp as fallback
    // The real persistence is handled when we have save_directory
    let _ = project; // will be saved via the renderer's save flow
}

fn remove_local_sync_project(id: &str) {
    let _ = id; // will be handled via the renderer's save flow
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
