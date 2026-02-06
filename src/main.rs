use std::sync::mpsc;
use tokio::sync::mpsc as tokio_mpsc;

mod app_state;
mod files;
mod status;
mod tailscale;
mod ui;

use app_state::{TailscaleCommand, TailscaleEvent};

fn main() -> eframe::Result<()> {
    egui_logger::builder()
    .max_level(log::LevelFilter::Info)
    .init()
    .unwrap();

    // Create channels for communication between UI and background task
    let (event_tx, event_rx) = mpsc::channel::<TailscaleEvent>();
    let (command_tx, command_rx) = tokio_mpsc::unbounded_channel::<TailscaleCommand>();

    // Shared status for the HTTP status endpoint
    let shared_status = status::new_shared_status();

    // Spawn the tokio runtime in a separate thread for the background task
    let event_tx_clone = event_tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            // Run the tailscale watcher
            if let Err(e) = tailscale::run_tailscale_backend(event_tx_clone, command_rx, shared_status).await {
                log::error!("Tailscale backend error: {:?}", e);
            }
        });
    });

    eframe::run_native(
        &format!("Tailscale Drive - {}", env!("CARGO_PKG_VERSION")),
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 750.0])
                .with_drag_and_drop(true),
            ..Default::default()
        },
        Box::new(move |cc| {
            // setting the default theme I use on all my egui apps..
            match serde_json::from_str::<eframe::egui::Style>(crate::app_state::STYLE) {
                Ok(theme) => {
                    let style = std::sync::Arc::new(theme);
                    cc.egui_ctx.set_style(style);
                }
                Err(e) => log::error!("Error setting theme: {e:?}")
            };
            let mut app = app_state::TailscaleDriveApp::new(cc);
            app.set_channels(event_rx, command_tx.clone());
            Ok(Box::new(app))
        }),
    )
}


