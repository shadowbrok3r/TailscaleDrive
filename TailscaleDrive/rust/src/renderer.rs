use std::ffi::c_void;

use egui::{pos2, vec2, Color32, RichText};
use egui_wgpu_backend::{RenderPass as EguiWgpuRenderer, ScreenDescriptor};

use crate::tailscale_client::{format_size, format_timestamp, TailscaleClient};

const DEFAULT_SERVER_URL: &str = "http://manjaro-work.taile483f.ts.net:8080";

// ── Page enum ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Monitor,
    ProjectSync,
}

pub struct Renderer {
    // wgpu
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,

    // egui
    egui_ctx: egui::Context,
    egui_rpass: EguiWgpuRenderer,
    pixels_per_point: f32,
    pending_events: Vec<egui::Event>,

    // TailscaleDrive client
    client: TailscaleClient,

    // UI state
    server_url_input: String,
    selected_file_idx: Option<usize>,
    theme_applied: bool,
    current_page: Page,
    browse_path_input: String,
    browse_fetched: bool,
    selected_remote_idx: Option<usize>,
    auto_browsed: bool,

    // Notification queue: (title, body)
    pending_notifications: Vec<(String, String)>,
    last_known_received: Option<String>,
    last_known_sent_name: Option<String>,

    // iOS keyboard state
    wants_keyboard: bool,
}

impl Renderer {
    pub fn new(
        layer_ptr: *mut c_void,
        width_px: u32,
        height_px: u32,
        pixels_per_point: f32,
    ) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });

        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer_ptr))
                .expect("create_surface_unsafe(CoreAnimationLayer)")
        };

        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            }))
            .expect("request_adapter");

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                memory_hints: wgpu::MemoryHints::default(),
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                trace: wgpu::Trace::Off,
            }))
            .expect("request_device");

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width_px.max(1),
            height: height_px.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        let egui_rpass = EguiWgpuRenderer::new(&device, format, 1);

        let client = TailscaleClient::new(DEFAULT_SERVER_URL);

        Self {
            device,
            queue,
            surface,
            config,

            egui_ctx,
            egui_rpass,
            pixels_per_point: pixels_per_point.max(0.5),
            pending_events: Vec::new(),

            client,
            server_url_input: DEFAULT_SERVER_URL.to_string(),
            selected_file_idx: None,
            theme_applied: false,
            current_page: Page::Monitor,
            browse_path_input: String::new(),
            browse_fetched: false,
            selected_remote_idx: None,
            auto_browsed: false,

            pending_notifications: Vec::new(),
            last_known_received: None,
            last_known_sent_name: None,
            wants_keyboard: false,
        }
    }

    pub fn set_pixels_per_point(&mut self, ppp: f32) {
        self.pixels_per_point = ppp.max(0.5);
    }

    /// Set the directory where downloaded/pulled files are saved (iOS Documents dir).
    pub fn set_save_directory(&mut self, path: &str) {
        self.client.save_directory = Some(path.to_string());
    }

    /// Returns true when there's a newly-saved file ready for the iOS share sheet.
    pub fn has_pending_share(&self) -> bool {
        !self.client.pending_share_paths.is_empty()
    }

    /// Pops and returns the full path to the next file ready for sharing.
    pub fn consume_pending_share_path(&mut self) -> String {
        if self.client.pending_share_paths.is_empty() {
            String::new()
        } else {
            self.client.pending_share_paths.remove(0)
        }
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32) {
        if width_px == 0 || height_px == 0 {
            return;
        }
        self.config.width = width_px;
        self.config.height = height_px;
        self.surface.configure(&self.device, &self.config);
    }

    // Touch input is in points
    pub fn touch_began(&mut self, x_pt: f32, y_pt: f32) {
        let pos = pos2(x_pt, y_pt);
        self.pending_events.push(egui::Event::PointerMoved(pos));
        self.pending_events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
    }

    pub fn touch_moved(&mut self, x_pt: f32, y_pt: f32) {
        self.pending_events
            .push(egui::Event::PointerMoved(pos2(x_pt, y_pt)));
    }

    pub fn touch_ended(&mut self, x_pt: f32, y_pt: f32) {
        let pos = pos2(x_pt, y_pt);
        self.pending_events.push(egui::Event::PointerMoved(pos));
        self.pending_events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        });
        // Tell egui the finger left the screen — fixes "stuck" button highlights
        self.pending_events.push(egui::Event::PointerGone);
    }

    // ── Notification API (called from Swift via bridge) ─────────────────

    pub fn has_pending_notification(&self) -> bool {
        !self.pending_notifications.is_empty()
    }

    pub fn notification_title(&self) -> String {
        self.pending_notifications
            .first()
            .map(|(t, _)| t.clone())
            .unwrap_or_default()
    }

    /// Returns the body AND consumes (pops) the front notification.
    pub fn consume_notification_body(&mut self) -> String {
        if self.pending_notifications.is_empty() {
            String::new()
        } else {
            let (_, body) = self.pending_notifications.remove(0);
            body
        }
    }

    // ── iOS Keyboard API (called from Swift via bridge) ───────────────

    /// Returns true when an egui text edit has focus and wants keyboard input.
    pub fn wants_keyboard(&self) -> bool {
        self.wants_keyboard
    }

    /// Called from Swift's UIKeyInput.insertText — sends typed text to egui.
    pub fn insert_text(&mut self, text: &str) {
        if text == "\n" {
            // Enter / Return key
            self.pending_events.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
        } else if text == "\t" {
            self.pending_events.push(egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
        } else {
            self.pending_events
                .push(egui::Event::Text(text.to_string()));
        }
    }

    /// Called from Swift's UIKeyInput.deleteBackward — sends backspace to egui.
    pub fn delete_backward(&mut self) {
        self.pending_events.push(egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
    }

    // ── Main render ─────────────────────────────────────────────────────

    pub fn render(&mut self, time_seconds: f64) {
        // Apply theme once
        if !self.theme_applied {
            self.theme_applied = true;
            apply_theme(&self.egui_ctx);
        }

        // Process network events from the background thread
        self.client.process_events();

        // Auto-browse server CWD once connected
        if self.client.connected && !self.auto_browsed {
            if let Some(ref cwd) = self.client.server_cwd {
                self.browse_path_input = cwd.clone();
                self.client.browse(Some(cwd.clone()));
                self.auto_browsed = true;
                self.browse_fetched = true;
            }
        }

        // ── Notification detection ──
        // Check for new received files
        if self.client.last_received_file != self.last_known_received {
            if let Some(ref name) = self.client.last_received_file {
                if self.last_known_received.is_some() {
                    // Not the initial load — a genuinely new file
                    self.pending_notifications.push((
                        "File Ready".to_string(),
                        format!("Tap to download: {}", name),
                    ));
                }
            }
            self.last_known_received = self.client.last_received_file.clone();
        }

        // Check for new sent files — only track when succeeded to avoid
        // missing the notification (status updates while sending would set the
        // name but succeeded=false, then when it flips to true the name matches
        // and we'd skip the notification).
        if let Some(ref sent) = self.client.last_sent {
            if sent.succeeded {
                let sent_name = Some(sent.name.clone());
                if sent_name != self.last_known_sent_name {
                    if self.last_known_sent_name.is_some() {
                        self.pending_notifications.push((
                            "File Sent".to_string(),
                            format!("Desktop sent: {}", sent.name),
                        ));
                    }
                    self.last_known_sent_name = sent_name;
                }
            }
        }

        // Validate selected file index
        if let Some(idx) = self.selected_file_idx {
            if idx >= self.client.waiting_files.len() {
                self.selected_file_idx = None;
            }
        }

        // ── egui frame ──
        let width_px = self.config.width;
        let height_px = self.config.height;
        let width_pt = (width_px as f32) / self.pixels_per_point;
        let height_pt = (height_px as f32) / self.pixels_per_point;

        let mut viewport_info = egui::ViewportInfo::default();
        viewport_info.native_pixels_per_point = Some(self.pixels_per_point);

        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(egui::ViewportId::ROOT, viewport_info);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::new(0.0, 0.0),
                vec2(width_pt, height_pt),
            )),
            time: Some(time_seconds),
            events: std::mem::take(&mut self.pending_events),
            viewports,
            ..Default::default()
        };

        // Deferred reconnect (requires &mut self.client AFTER run completes)
        let mut reconnect_url: Option<String> = None;

        // Clone the context (cheap Arc clone) to avoid borrow conflict
        let ctx_clone = self.egui_ctx.clone();
        let full_output = ctx_clone.run(raw_input, |ctx| {
            // Deferred actions
            let mut file_to_download: Option<String> = None;
            let mut do_download_last = false;
            let mut do_refresh = false;
            let mut do_browse: Option<Option<String>> = None;
            let mut file_to_pull: Option<String> = None;

            // ═══════════════════════════════════════════════════
            //  TOP BAR
            // ═══════════════════════════════════════════════════
            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                ui.add_space(50.0);
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("📡 Tailscale Drive").strong());
                    ui.separator();

                    let (color, text) = if self.client.connected {
                        (Color32::from_rgb(46, 204, 113), "Connected")
                    } else {
                        (Color32::from_rgb(231, 76, 60), "Disconnected")
                    };
                    ui.colored_label(color, format!("○ {}", text));

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("⟳ Refresh").clicked() {
                                do_refresh = true;
                            }
                        },
                    );
                });

                ui.label(
                    RichText::new(&self.client.status_message)
                        .weak()
                        .small(),
                );

                ui.add_space(4.0);

                // ── Page tabs ──
                ui.horizontal(|ui| {
                    let monitor_selected = self.current_page == Page::Monitor;
                    let sync_selected = self.current_page == Page::ProjectSync;

                    if ui
                        .selectable_label(
                            monitor_selected,
                            RichText::new("📡 Monitor").strong(),
                        )
                        .clicked()
                    {
                        self.current_page = Page::Monitor;
                    }

                    ui.separator();

                    if ui
                        .selectable_label(
                            sync_selected,
                            RichText::new("🔄 Project Sync").strong(),
                        )
                        .clicked()
                    {
                        self.current_page = Page::ProjectSync;
                        if !self.browse_fetched {
                            do_browse = Some(None);
                            self.browse_fetched = true;
                        }
                    }
                });

                ui.add_space(2.0);
            });

            // ═══════════════════════════════════════════════════
            //  CENTRAL PANEL
            // ═══════════════════════════════════════════════════
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.current_page {
                            Page::Monitor => {
                                self.draw_monitor_page(
                                    ui,
                                    &mut reconnect_url,
                                    &mut file_to_download,
                                    &mut do_download_last,
                                );
                            }
                            Page::ProjectSync => {
                                self.draw_project_sync_page(
                                    ui,
                                    &mut do_browse,
                                    &mut file_to_pull,
                                );
                            }
                        }
                    });
            });

            // Apply deferred actions
            if let Some(ref name) = file_to_download {
                self.client.download_file(name);
            }
            if do_download_last {
                self.client.download_last();
            }
            if do_refresh {
                self.client.refresh();
            }
            if let Some(path) = do_browse {
                self.client.browse(path);
            }
            if let Some(ref name) = file_to_pull {
                self.client.pull_file(name);
            }
        });

        // Update keyboard state — use wants_keyboard_input() which checks
        // whether any TextEdit has focus, instead of mutable_text_under_cursor
        // which only checks if the pointer hovers over mutable text (goes false
        // after PointerGone on touch-up, causing the keyboard to dismiss).
        self.wants_keyboard = self.egui_ctx.wants_keyboard_input();

        // Handle reconnect after run() (needs &mut self.client)
        if let Some(url) = reconnect_url {
            self.client = TailscaleClient::new(&url);
            self.browse_fetched = false;
            self.auto_browsed = false;
            self.selected_remote_idx = None;
        }

        // ── Tessellate & render ──
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            physical_width: width_px,
            physical_height: height_px,
            scale_factor: self.pixels_per_point,
        };

        self.egui_rpass
            .add_textures(&self.device, &self.queue, &full_output.textures_delta)
            .expect("add_textures");

        self.egui_rpass.update_buffers(
            &self.device,
            &self.queue,
            &paint_jobs,
            &screen_descriptor,
        );

        // ── wgpu frame ──
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[egui-renderer] get_current_texture error: {:?}", e);
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Clear colour = same dark panel_fill (rgb 12 12 15)
        self.egui_rpass
            .execute(
                &mut encoder,
                &view,
                &paint_jobs,
                &screen_descriptor,
                Some(wgpu::Color {
                    r: 0.047,
                    g: 0.047,
                    b: 0.059,
                    a: 1.0,
                }),
            )
            .expect("egui render");

        self.queue.submit(Some(encoder.finish()));
        frame.present();

        self.egui_rpass
            .remove_textures(full_output.textures_delta)
            .expect("remove_textures");
    }

    // ═══════════════════════════════════════════════════════════════════
    //  PAGE 1: MONITOR
    // ═══════════════════════════════════════════════════════════════════

    fn draw_monitor_page(
        &mut self,
        ui: &mut egui::Ui,
        reconnect_url: &mut Option<String>,
        file_to_download: &mut Option<String>,
        do_download_last: &mut bool,
    ) {
        // ─── Server Config ───
        ui.group(|ui| {
            ui.label(
                RichText::new("SERVER")
                    .strong()
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("URL:");
                let re = ui.text_edit_singleline(&mut self.server_url_input);
                if self.server_url_input != self.client.server_url {
                    if ui.button("Connect").clicked()
                        || (re.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        *reconnect_url = Some(self.server_url_input.clone());
                    }
                }
            });
        });

        ui.add_space(8.0);

        // ─── Desktop Transfer Activity ───
        ui.group(|ui| {
            ui.label(
                RichText::new("DESKTOP ACTIVITY")
                    .strong()
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);

            // Last file the desktop SENT (to any peer)
            ui.label(RichText::new("Last Sent from Desktop:").small().color(Color32::GRAY));
            if let Some(ref sent) = self.client.last_sent {
                let icon = if sent.sending {
                    "⏳"
                } else if sent.succeeded {
                    "✅"
                } else {
                    "❌"
                };
                ui.label(format!(
                    "{} {} ({})",
                    icon,
                    sent.name,
                    format_size(sent.size)
                ));
            } else {
                ui.label(RichText::new("None yet").weak());
            }

            ui.separator();

            // Last file the desktop RECEIVED (via Taildrop from another device)
            ui.label(RichText::new("Last Received on Desktop:").small().color(Color32::GRAY));
            if let Some(ref name) = self.client.last_received_file {
                ui.horizontal(|ui| {
                    ui.label(format!("📄 {}", name));
                    if ui.button("💾 Save to iPhone").clicked() {
                        *do_download_last = true;
                    }
                });
            } else {
                ui.label(RichText::new("None yet").weak());
            }

            // Download status toast
            if let Some(ref status) = self.client.download_status {
                ui.add_space(4.0);
                let color = if status.starts_with('✓') {
                    Color32::from_rgb(46, 204, 113)
                } else {
                    Color32::from_rgb(231, 76, 60)
                };
                ui.colored_label(color, status.as_str());
            }
        });

        ui.add_space(8.0);

        // ─── Waiting Files (desktop Taildrop inbox) ───
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("DESKTOP TAILDROP INBOX")
                        .strong()
                        .small()
                        .color(Color32::GRAY),
                );
                ui.label(
                    RichText::new(format!("({})", self.client.waiting_files.len()))
                        .weak()
                        .small(),
                );
            });
            ui.add_space(4.0);

            if self.client.waiting_files.is_empty() {
                ui.label(RichText::new("No files in desktop inbox").weak());
            } else {
                for (idx, file) in self.client.waiting_files.iter().enumerate() {
                    let is_selected = self.selected_file_idx == Some(idx);

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    is_selected,
                                    RichText::new(format!("📄 {}", file.name)).strong(),
                                )
                                .clicked()
                            {
                                self.selected_file_idx = Some(idx);
                            }
                            ui.label(
                                RichText::new(format_size(file.size)).weak().small(),
                            );
                        });

                        if is_selected {
                            if ui.button("💾 Save to iPhone").clicked() {
                                *file_to_download = Some(file.name.clone());
                            }
                        }
                    });
                }
            }
        });
    }

    // ═══════════════════════════════════════════════════════════════════
    //  PAGE 2: FILE BROWSER  (remote desktop filesystem)
    // ═══════════════════════════════════════════════════════════════════

    fn draw_project_sync_page(
        &mut self,
        ui: &mut egui::Ui,
        do_browse: &mut Option<Option<String>>,
        file_to_pull: &mut Option<String>,
    ) {
        ui.heading("File Browser");
        ui.add_space(4.0);

        // ─── Navigation bar ───
        ui.horizontal(|ui| {
            // Up button
            if ui.button("⬆ Up").clicked() {
                if let Some(pos) = self.browse_path_input.rfind('/') {
                    self.browse_path_input.truncate(pos);
                    if self.browse_path_input.is_empty() {
                        self.browse_path_input = "/".to_string();
                    }
                } else {
                    self.browse_path_input = "/".to_string();
                }
                *do_browse = Some(Some(self.browse_path_input.clone()));
                self.selected_remote_idx = None;
            }

            // Home button (server CWD)
            if ui.button("🏠").clicked() {
                if let Some(ref cwd) = self.client.server_cwd {
                    self.browse_path_input = cwd.clone();
                    *do_browse = Some(Some(cwd.clone()));
                    self.selected_remote_idx = None;
                }
            }

            // Refresh button
            if ui.button("⟳").clicked() {
                let path = if self.browse_path_input.is_empty() {
                    None
                } else {
                    Some(self.browse_path_input.clone())
                };
                *do_browse = Some(path);
            }

            ui.separator();

            // Path text edit
            let re = ui.add(
                egui::TextEdit::singleline(&mut self.browse_path_input)
                    .desired_width(ui.available_width()),
            );
            if re.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let path = if self.browse_path_input.is_empty() {
                    None
                } else {
                    Some(self.browse_path_input.clone())
                };
                *do_browse = Some(path);
                self.selected_remote_idx = None;
            }
        });

        // ─── Status / toast ───
        if let Some(ref status) = self.client.browse_status {
            let color = if status.starts_with('✓') {
                Color32::from_rgb(46, 204, 113)
            } else {
                Color32::GRAY
            };
            ui.colored_label(color, status.as_str());
        }

        ui.separator();

        // ─── Directory contents ───
        if self.client.remote_files.is_empty() && !self.client.connected {
            ui.label(RichText::new("Not connected — waiting for server…").weak());
        } else if self.client.remote_files.is_empty() {
            ui.label(RichText::new("Empty directory").weak());
        } else {
            // Collect directories and files, sorted
            let mut dir_indices: Vec<usize> = Vec::new();
            let mut file_indices: Vec<usize> = Vec::new();
            for (i, f) in self.client.remote_files.iter().enumerate() {
                if f.is_dir {
                    dir_indices.push(i);
                } else {
                    file_indices.push(i);
                }
            }
            dir_indices.sort_by(|&a, &b| {
                self.client.remote_files[a]
                    .name
                    .to_lowercase()
                    .cmp(&self.client.remote_files[b].name.to_lowercase())
            });
            file_indices.sort_by(|&a, &b| {
                self.client.remote_files[a]
                    .name
                    .to_lowercase()
                    .cmp(&self.client.remote_files[b].name.to_lowercase())
            });

            // Build combined sorted index list: dirs first, then files
            let sorted: Vec<usize> = dir_indices
                .iter()
                .chain(file_indices.iter())
                .copied()
                .collect();

            let mut nav_to: Option<String> = None;

            for &idx in &sorted {
                let entry = &self.client.remote_files[idx];
                let is_selected = self.selected_remote_idx == Some(idx);
                let icon = if entry.is_dir { "📂" } else { "📄" };

                let label_text = if entry.is_dir {
                    format!("{} {}/", icon, entry.name)
                } else {
                    format!(
                        "{} {} ({})",
                        icon,
                        entry.name,
                        format_size(entry.size as u64)
                    )
                };

                let response =
                    ui.selectable_label(is_selected, RichText::new(&label_text));

                if response.clicked() {
                    if entry.is_dir {
                        // Navigate into directory
                        let new_path = if self.browse_path_input.is_empty()
                            || self.browse_path_input == "/"
                        {
                            format!("/{}", entry.name)
                        } else {
                            format!("{}/{}", self.browse_path_input, entry.name)
                        };
                        nav_to = Some(new_path);
                    } else {
                        // Select the file
                        self.selected_remote_idx = Some(idx);
                    }
                }
            }

            // Apply directory navigation (deferred to avoid borrow conflicts)
            if let Some(new_path) = nav_to {
                self.browse_path_input = new_path.clone();
                *do_browse = Some(Some(new_path));
                self.selected_remote_idx = None;
            }
        }

        ui.add_space(8.0);

        // ─── Action buttons for selected file ───
        if let Some(sel_idx) = self.selected_remote_idx {
            if let Some(selected) = self.client.remote_files.get(sel_idx) {
                if !selected.is_dir {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("📄 {}", selected.name)).strong(),
                            );
                            ui.label(
                                RichText::new(format_size(selected.size as u64))
                                    .weak()
                                    .small(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Modified: {}",
                                    format_timestamp(selected.modified)
                                ))
                                .weak()
                                .small(),
                            );
                        });
                        if ui.button("📥 Pull File").clicked() {
                            // Build the full path for the pull request
                            let full_path = if self.browse_path_input.is_empty()
                                || self.browse_path_input == "/"
                            {
                                format!("/{}", selected.name)
                            } else {
                                format!("{}/{}", self.browse_path_input, selected.name)
                            };
                            *file_to_pull = Some(full_path);
                        }
                    });
                }
            }
        }
    }
}

// ── Theme — exact desktop TailscaleDrive style from app_state::STYLE ────

/// Serialised egui::Style from the desktop TailscaleDrive app.
const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":false,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;

fn apply_theme(ctx: &egui::Context) {
    match serde_json::from_str::<egui::Style>(STYLE) {
        Ok(mut theme) => {
            theme.override_font_id = Some(egui::FontId::new(15., egui::FontFamily::Proportional));
            ctx.set_style(std::sync::Arc::new(theme));
        }
        Err(e) => {
            eprintln!("[theme] serde_json deserialize failed: {e}");
            let mut style = (*ctx.style()).clone();
            style.override_font_id = Some(egui::FontId::new(15., egui::FontFamily::Proportional));
            style.visuals.dark_mode = true;
            style.visuals.panel_fill = Color32::from_rgb(12, 12, 15);
            style.visuals.window_fill = Color32::from_rgb(11, 11, 15);
            style.visuals.override_text_color =
                Some(Color32::from_rgb(207, 216, 220));
            style.visuals.faint_bg_color = Color32::from_rgb(17, 18, 22);
            style.visuals.extreme_bg_color =
                Color32::from_rgba_premultiplied(9, 12, 15, 83);
            style.visuals.code_bg_color = Color32::from_rgb(30, 31, 35);
            style.visuals.hyperlink_color = Color32::from_rgb(135, 85, 129);
            style.visuals.window_stroke = egui::Stroke::new(
                1.0,
                Color32::from_rgba_premultiplied(77, 94, 120, 138),
            );
            style.visuals.selection.bg_fill =
                Color32::from_rgba_premultiplied(23, 64, 53, 27);

            style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.noninteractive.fg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(207, 216, 220));
            style.visuals.widgets.inactive.weak_bg_fill =
                Color32::from_rgba_premultiplied(8, 8, 8, 231);
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.5, Color32::from_rgb(48, 51, 73));
            style.visuals.widgets.hovered.bg_fill =
                Color32::from_rgba_premultiplied(37, 29, 61, 97);
            style.visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.7, Color32::from_rgb(106, 101, 155));
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(12, 12, 15);

            let cr = egui::CornerRadius::same(6);
            style.visuals.window_corner_radius = cr;
            style.visuals.menu_corner_radius = cr;
            style.visuals.widgets.noninteractive.corner_radius = cr;
            style.visuals.widgets.inactive.corner_radius = cr;
            style.visuals.widgets.hovered.corner_radius = cr;
            style.visuals.widgets.active.corner_radius = cr;

            ctx.set_style(style);
        }
    }
}
