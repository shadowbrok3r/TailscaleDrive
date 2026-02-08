use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, post, put},
};
use serde::{Deserialize as SerdeDeserialize, Serialize};
use tokio_util::io::ReaderStream;

// --- Shared State ---

#[derive(Debug, Clone, Serialize, Default)]
pub struct SentFileInfo {
    pub name: String,
    pub peer_id: String,
    pub size: u64,
    pub timestamp: u64,
    pub succeeded: bool,
    pub sending: bool,
}

/// Tracks received files and their FinalPaths for the download endpoint.
#[derive(Default)]
pub struct ReceivedState {
    /// Name of the most recently received file
    pub last_file: Option<String>,
    /// Maps filename → FinalPath on disk (from IncomingFiles)
    pub file_paths: HashMap<String, PathBuf>,
}

/// Combined shared state for the HTTP server and backend.
#[derive(Clone)]
pub struct AppState {
    pub last_sent: Arc<Mutex<Option<SentFileInfo>>>,
    pub received: Arc<Mutex<ReceivedState>>,
    pub peers: Arc<Mutex<Vec<crate::app_state::TailscalePeer>>>,
    pub sync_projects: Arc<Mutex<Vec<crate::app_state::SyncProject>>>,
}

pub fn new_app_state() -> AppState {
    let projects = load_sync_projects();
    AppState {
        last_sent: Arc::new(Mutex::new(None)),
        received: Arc::new(Mutex::new(ReceivedState::default())),
        peers: Arc::new(Mutex::new(Vec::new())),
        sync_projects: Arc::new(Mutex::new(projects)),
    }
}

// --- Sync project persistence ---

fn sync_projects_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("tailscale-drive")
        .join("sync_projects.json")
}

pub fn load_sync_projects() -> Vec<crate::app_state::SyncProject> {
    let path = sync_projects_path();
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_sync_projects(projects: &[crate::app_state::SyncProject]) {
    let path = sync_projects_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(projects) {
        let _ = std::fs::write(&path, data);
    }
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// --- Handlers ---

/// GET /status — JSON status with last sent/received file info
async fn status_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sent = state.last_sent.lock().unwrap().clone();
    let last_received = state.received.lock().unwrap().last_file.clone();
    let server_cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    Json(serde_json::json!({
        "last_sent_file": sent,
        "last_received_file": last_received,
        "server_cwd": server_cwd,
    }))
}

/// GET /files — list all files waiting in the Taildrop inbox
async fn list_files_handler() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let files = crate::files::list_waiting_files()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to list files: {}", e)))?;

    let result: Vec<serde_json::Value> = files
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "size": f.size,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "files": result })))
}

/// GET /download/:name — download a specific file by name.
/// Streams from FinalPath on disk if known, otherwise buffers from the tailscaled API.
async fn download_file_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response<Body>, (StatusCode, String)> {
    // Check if we have a local FinalPath for this file
    let local_path = {
        let received = state.received.lock().unwrap();
        received.file_paths.get(&name).cloned()
    };

    // Try streaming from disk (efficient for large files)
    if let Some(ref path) = local_path {
        if let Ok(file) = tokio::fs::File::open(path).await {
            let metadata = file.metadata().await.ok();
            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);

            let mut builder = Response::builder()
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", name),
                );

            if let Some(meta) = metadata {
                builder = builder.header(header::CONTENT_LENGTH, meta.len());
            }

            return builder
                .body(body)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
    }

    // Fallback: download from tailscaled API (buffered)
    let content = crate::files::download_received_file(&name)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                format!("File '{}' not available: {}", name, e),
            )
        })?;

    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", name),
        )
        .header(header::CONTENT_LENGTH, content.len())
        .body(Body::from(content))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// GET /download — download the most recently received file
async fn download_last_handler(
    State(state): State<AppState>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let name = {
        let received = state.received.lock().unwrap();
        received
            .last_file
            .clone()
            .ok_or((StatusCode::NOT_FOUND, "No file received yet".to_string()))?
    };

    download_file_handler(State(state), Path(name)).await
}

/// GET /peers — list all Tailscale peers on the network
async fn peers_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let peers = state.peers.lock().unwrap();
    let result: Vec<serde_json::Value> = peers
        .iter()
        .filter(|p| !p.is_self)
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "hostname": p.hostname,
                "dns_name": p.dns_name,
                "ip_addresses": p.ip_addresses,
                "online": p.online,
                "os": p.os,
            })
        })
        .collect();
    Json(serde_json::json!(result))
}

// --- Browse / Upload ---

#[derive(SerdeDeserialize)]
struct BrowseQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct RemoteFileInfo {
    name: String,
    is_dir: bool,
    size: i64,
    modified: u64,
}

/// GET /browse?path=<optional> — list files in a directory (defaults to $HOME).
async fn browse_handler(
    Query(params): Query<BrowseQuery>,
) -> Result<Json<Vec<RemoteFileInfo>>, (StatusCode, String)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let base = params.path.unwrap_or(home);
    let base_path = std::path::PathBuf::from(&base);

    if !base_path.exists() || !base_path.is_dir() {
        return Err((StatusCode::NOT_FOUND, "Directory not found".to_string()));
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                files.push(RemoteFileInfo {
                    name,
                    is_dir: metadata.is_dir(),
                    size: metadata.len() as i64,
                    modified,
                });
            }
        }
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(files))
}

/// GET /pull?path=<filepath> — download an arbitrary file from the server's filesystem
async fn pull_file_handler(
    Query(params): Query<BrowseQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let path_str = params
        .path
        .ok_or((StatusCode::BAD_REQUEST, "Missing path parameter".to_string()))?;
    let file_path = std::path::PathBuf::from(&path_str);

    if !file_path.exists() || !file_path.is_file() {
        return Err((StatusCode::NOT_FOUND, format!("File not found: {}", path_str)));
    }

    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let metadata = file.metadata().await.ok();
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        );

    if let Some(meta) = metadata {
        builder = builder.header(header::CONTENT_LENGTH, meta.len());
    }

    builder
        .body(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// PUT /upload/{*path} — upload a file (raw body bytes) to the given path relative to $HOME.
async fn upload_handler(
    Path(file_path): Path<String>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let dest = std::path::PathBuf::from(&home).join(&file_path);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    std::fs::write(&dest, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    log::info!("Uploaded: {}", file_path);
    Ok(StatusCode::OK)
}

// --- Sync endpoints ---

/// GET /sync/projects — list all sync projects
async fn sync_list_projects(
    State(state): State<AppState>,
) -> Json<Vec<crate::app_state::SyncProject>> {
    let projects = state.sync_projects.lock().unwrap();
    Json(projects.clone())
}

#[derive(SerdeDeserialize)]
struct CreateSyncProjectRequest {
    local_path: String,
    remote_path: String,
}

/// POST /sync/projects — create a new sync project
async fn sync_create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateSyncProjectRequest>,
) -> Result<Json<crate::app_state::SyncProject>, (StatusCode, String)> {
    let project = crate::app_state::SyncProject {
        id: format!("{:x}", rand_id()),
        local_path: body.local_path,
        remote_path: body.remote_path,
        last_synced: unix_timestamp(),
        paused: false,
    };

    let mut projects = state.sync_projects.lock().unwrap();
    projects.push(project.clone());
    save_sync_projects(&projects);
    log::info!("Created sync project: {} -> {}", project.local_path, project.remote_path);
    Ok(Json(project))
}

/// DELETE /sync/projects/{id} — remove a sync project
async fn sync_delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut projects = state.sync_projects.lock().unwrap();
    let before = projects.len();
    projects.retain(|p| p.id != id);
    if projects.len() == before {
        return Err((StatusCode::NOT_FOUND, format!("Project '{}' not found", id)));
    }
    save_sync_projects(&projects);
    log::info!("Deleted sync project: {}", id);
    Ok(StatusCode::OK)
}

#[derive(Serialize)]
struct SyncChangeResponse {
    id: String,
    remote_path: String,
    local_path: String,
    new_modified: u64,
}

/// GET /sync/check — return projects where the desktop file has been modified since last sync
async fn sync_check(
    State(state): State<AppState>,
) -> Json<Vec<SyncChangeResponse>> {
    let projects = state.sync_projects.lock().unwrap();
    let mut changes = Vec::new();
    for project in projects.iter() {
        if project.paused {
            continue;
        }
        let path = std::path::Path::new(&project.local_path);
        if let Ok(metadata) = std::fs::metadata(path) {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if modified > project.last_synced {
                changes.push(SyncChangeResponse {
                    id: project.id.clone(),
                    remote_path: project.remote_path.clone(),
                    local_path: project.local_path.clone(),
                    new_modified: modified,
                });
            }
        }
    }
    Json(changes)
}

#[derive(SerdeDeserialize)]
struct SyncAckRequest {
    id: String,
    timestamp: u64,
}

/// POST /sync/ack — iOS confirms it pulled a file; updates last_synced
async fn sync_ack(
    State(state): State<AppState>,
    Json(body): Json<SyncAckRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut projects = state.sync_projects.lock().unwrap();
    if let Some(project) = projects.iter_mut().find(|p| p.id == body.id) {
        project.last_synced = body.timestamp;
        save_sync_projects(&projects);
        Ok(StatusCode::OK)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Project '{}' not found", body.id)))
    }
}

#[derive(SerdeDeserialize)]
struct SyncUploadQuery {
    path: String,
}

/// PUT /sync/upload?path=<absolute_path> — upload a file to an absolute path on the desktop
async fn sync_upload_handler(
    Query(params): Query<SyncUploadQuery>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let dest = std::path::PathBuf::from(&params.path);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    std::fs::write(&dest, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    log::info!("Sync upload: {}", params.path);
    Ok(StatusCode::OK)
}

/// Simple random ID generator (no external crate needed)
fn rand_id() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    hasher.finish()
}

// --- Server ---

pub async fn run_status_server(state: AppState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(status_handler))
        .route("/files", get(list_files_handler))
        .route("/download", get(download_last_handler))
        .route("/download/{name}", get(download_file_handler))
        .route("/browse", get(browse_handler))
        .route("/pull", get(pull_file_handler))
        .route("/upload/{*path}", put(upload_handler))
        .route("/peers", get(peers_handler))
        .route("/sync/projects", get(sync_list_projects).post(sync_create_project))
        .route("/sync/projects/{id}", delete(sync_delete_project))
        .route("/sync/check", get(sync_check))
        .route("/sync/ack", post(sync_ack))
        .route("/sync/upload", put(sync_upload_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    log::info!("Status server listening on 0.0.0.0:8080");
    axum::serve(listener, app).await?;
    Ok(())
}
