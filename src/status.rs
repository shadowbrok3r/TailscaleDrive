use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use serde::Serialize;
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
}

pub fn new_app_state() -> AppState {
    AppState {
        last_sent: Arc::new(Mutex::new(None)),
        received: Arc::new(Mutex::new(ReceivedState::default())),
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
    Json(serde_json::json!({
        "last_sent_file": sent,
        "last_received_file": last_received,
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

// --- Server ---

pub async fn run_status_server(state: AppState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(status_handler))
        .route("/files", get(list_files_handler))
        .route("/download", get(download_last_handler))
        .route("/download/{name}", get(download_file_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    log::info!("Status server listening on 0.0.0.0:8080");
    axum::serve(listener, app).await?;
    Ok(())
}
