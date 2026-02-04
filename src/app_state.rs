use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use tokio::sync::mpsc::UnboundedSender;

pub const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":false,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;

/// Tailscale peer/device on the network
#[derive(Debug, Clone)]
pub struct TailscalePeer {
    pub id: String,
    pub hostname: String,
    pub dns_name: String,
    pub ip_addresses: Vec<String>,
    pub online: bool,
    pub is_self: bool,
    pub os: String,
    pub can_receive_files: bool,
}

/// A file received via Taildrop
#[derive(Debug, Clone)]
pub struct ReceivedFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub from_peer: String,
    pub received_at: std::time::Instant,
    pub saved: bool,
}

/// A file currently being transferred
#[derive(Debug, Clone)]
pub struct TransferringFile {
    pub name: String,
    pub size: u64,
    pub transferred: u64,
    pub done: bool,
}

/// Messages sent from the background Tailscale watcher to the UI
#[derive(Debug)]
pub enum TailscaleEvent {
    /// Updated list of peers
    PeersUpdated(Vec<TailscalePeer>),
    /// A file transfer completed
    FileReceived(ReceivedFile),
    /// A file is being transferred (progress update)
    FileTransferring(TransferringFile),
    /// Connection status changed
    ConnectionStatus(bool, String),
    /// Error occurred
    Error(String),
}

/// Commands sent from the UI to the background task
#[derive(Debug)]
pub enum TailscaleCommand {
    /// Taildrop a file
    SendFile { peer_id: String, file_path: PathBuf },
    /// Refresh Tailnet clients
    RefreshPeers,
    /// Delete a received file from the temp location
    DeleteReceivedFile(PathBuf),
}

pub struct TailscaleDriveApp {
    // Communication
    pub event_rx: Option<Receiver<TailscaleEvent>>,
    pub command_tx: Option<UnboundedSender<TailscaleCommand>>,

    // Connection state
    pub connected: bool,
    pub status_message: String,

    // Tailnet clients
    pub peers: Vec<TailscalePeer>,
    pub selected_peer: Option<String>,

    // Files
    pub received_files: Vec<ReceivedFile>,
    pub transferring_files: Vec<TransferringFile>,
    pub files_to_send: Vec<PathBuf>,

    // UI state
    pub search_query: String,
    pub show_offline_peers: bool,
    pub selected_received_file: Option<usize>,

    // File explorer state
    pub current_directory: PathBuf,
    pub directory_contents: Vec<DirectoryEntry>,
    pub selected_directory_item: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

impl TailscaleDriveApp {
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));

        let mut app = Self {
            event_rx: None,
            command_tx: None,
            connected: false,
            status_message: "Initializing...".to_string(),
            peers: Vec::new(),
            selected_peer: None,
            received_files: Vec::new(),
            transferring_files: Vec::new(),
            files_to_send: Vec::new(),
            search_query: String::new(),
            show_offline_peers: false,
            selected_received_file: None,
            current_directory: home,
            directory_contents: Vec::new(),
            selected_directory_item: None,
        };

        app.refresh_directory();
        app
    }

    pub fn set_channels(
        &mut self,
        event_rx: Receiver<TailscaleEvent>,
        command_tx: UnboundedSender<TailscaleCommand>,
    ) {
        self.event_rx = Some(event_rx);
        self.command_tx = Some(command_tx);
    }

    pub fn refresh_directory(&mut self) {
        self.directory_contents.clear();
        self.selected_directory_item = None;

        if let Ok(entries) = std::fs::read_dir(&self.current_directory) {
            let mut items: Vec<DirectoryEntry> = entries
                .filter_map(|e| e.ok())
                .filter_map(|entry| {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();

                    // TODO: Make this optional / checkbox in UI
                    if name.starts_with('.') {
                        return None;
                    }

                    let metadata = entry.metadata().ok()?;
                    Some(DirectoryEntry {
                        name,
                        path,
                        is_dir: metadata.is_dir(),
                        size: metadata.len(),
                    })
                })
                .collect();

            // Sort: directories first, then files, alphabetically
            items.sort_by(|a, b| {
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });

            self.directory_contents = items;
        }
    }

    pub fn navigate_to(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.current_directory = path;
            self.refresh_directory();
        }
    }

    pub fn navigate_up(&mut self) {
        if let Some(parent) = self.current_directory.parent() {
            self.current_directory = parent.to_path_buf();
            self.refresh_directory();
        }
    }

    pub fn process_events(&mut self) {
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    TailscaleEvent::PeersUpdated(peers) => {
                        self.peers = peers;
                    }
                    TailscaleEvent::FileReceived(file) => {
                        // Check if file already exists in list
                        if !self.received_files.iter().any(|f| f.path == file.path) {
                            self.received_files.push(file);
                        }
                    }
                    TailscaleEvent::FileTransferring(transfer) => {
                        // Update or add transfer progress
                        if let Some(existing) = self
                            .transferring_files
                            .iter_mut()
                            .find(|f| f.name == transfer.name)
                        {
                            *existing = transfer;
                        } else {
                            self.transferring_files.push(transfer);
                        }
                        // Remove completed transfers
                        self.transferring_files.retain(|f| !f.done);
                    }
                    TailscaleEvent::ConnectionStatus(connected, message) => {
                        self.connected = connected;
                        self.status_message = message;
                    }
                    TailscaleEvent::Error(err) => {
                        self.status_message = format!("Error: {}", err);
                    }
                }
            }
        }
    }

    pub fn send_command(&self, cmd: TailscaleCommand) {
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn _filtered_peers(&self) -> Vec<&TailscalePeer> {
        self.peers
            .iter()
            .filter(|p| {
                // Don't show self
                if p.is_self {
                    return false;
                }
                // Filter by online status
                if !self.show_offline_peers && !p.online {
                    return false;
                }
                // Filter by search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    return p.hostname.to_lowercase().contains(&query)
                        || p.dns_name.to_lowercase().contains(&query);
                }
                true
            })
            .collect()
    }
}