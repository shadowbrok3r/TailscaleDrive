use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct SentFileInfo {
    pub name: String,
    pub peer_id: String,
    pub size: u64,
    pub timestamp: u64,
    pub succeeded: bool,
    pub sending: bool,
}

pub type SharedStatus = Arc<Mutex<Option<SentFileInfo>>>;

pub fn new_shared_status() -> SharedStatus {
    Arc::new(Mutex::new(None))
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn status_handler(State(status): State<SharedStatus>) -> Json<serde_json::Value> {
    let info = status.lock().unwrap().clone();
    Json(serde_json::json!({
        "last_file": info
    }))
}

pub async fn run_status_server(status: SharedStatus) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(status_handler))
        .with_state(status);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    log::info!("Status server listening on 0.0.0.0:8080/status");
    axum::serve(listener, app).await?;
    Ok(())
}
