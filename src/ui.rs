use eframe::egui::{self, Color32, RichText, Sense, StrokeKind, Vec2};
use std::cmp::Ordering;
use std::path::PathBuf;

use super::app_state::TailscaleCommand;

impl eframe::App for super::app_state::TailscaleDriveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process events from background task
        self.process_events();

        // Request repaint to keep UI responsive
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // Handle dropped files
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    if !self.files_to_send.contains(path) {
                        self.files_to_send.push(path.clone());
                    }
                }
            }
        });

        eframe::egui::Window::new("Logs")
            .open(&mut self.show_logs)
            .show(ctx, |ui| 
            egui_logger::logger_ui().show(ui)
        );

        // Top bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tailscale Drive");
                ui.separator();

                // Connection status indicator
                let (status_color, status_text) = if self.connected {
                    (Color32::from_rgb(46, 204, 113), "Connected")
                } else {
                    (Color32::from_rgb(231, 76, 60), "Disconnected")
                };

                ui.colored_label(status_color, format!("○ {}", status_text));
                ui.separator();
                ui.label(&self.status_message);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⟳ Refresh").clicked() {
                        self.send_command(TailscaleCommand::RefreshPeers);
                    }
                    ui.separator();
                    let show_hide_logs = if self.show_logs { "Hide Logs" } else { "Show Logs" };
                    if ui.button(show_hide_logs).clicked() {
                        self.show_logs = !self.show_logs;
                    }
                });
            });
        });

        // Device list
        egui::SidePanel::left("devices_panel")
            .default_width(280.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Devices");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.search_query);
                });

                ui.checkbox(&mut self.show_offline_peers, "Show offline devices");
                ui.separator();

                // Device list
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.peers.sort_by(|a, b | {
                        match (a.online, b.online) {
                            (true, false) => Ordering::Less,
                            (false, true) => Ordering::Greater,
                            _ => {
                                a.hostname
                                    .to_lowercase()
                                    .cmp(&b.hostname.to_lowercase())
                            }
                        }
                    });

                    let peer_data: Vec<_> = self
                        .peers
                        .iter()
                        .filter(|p| {
                            if p.is_self {
                                return false;
                            }
                            if !self.show_offline_peers && !p.online {
                                return false;
                            }
                            if !self.search_query.is_empty() {
                                let query = self.search_query.to_lowercase();
                                return p.hostname.to_lowercase().contains(&query)
                                    || p.dns_name.to_lowercase().contains(&query);
                            }
                            true
                        })
                        .map(|p| (p.id.clone(), p.hostname.clone(), p.dns_name.clone(), 
                                  p.ip_addresses.clone(), p.online, p.os.clone()))
                        .collect();

                    if peer_data.is_empty() {
                        ui.label("No devices found");
                    } else {
                        let mut new_selection = None;

                        for (id, hostname, dns_name, ips, _online, os) in &peer_data {
                            // ui.horizontal(|ui| {

                            //     ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {

                            //     });
                            // });

                            let is_selected = self.selected_peer.as_ref() == Some(id);

                            let logo = match os.to_lowercase().as_str() {
                                "linux" => "🐧",
                                "macos" => "🍎",
                                "windows" => "🪟",
                                "android" => "📱",
                                "ios" => "🍎",
                                _ => "🖥",
                            };
                            let response = ui.selectable_label(
                                is_selected,
                                format!("{logo} {hostname}"),
                            );

                            if response.clicked() {
                                new_selection = Some(id.clone());
                            }

                            if response.secondary_clicked() {
                                if ips.len() >= 1 {
                                    ctx.copy_text(ips[0].clone());
                                }
                            }

                            // Show IP on hover
                            response.on_hover_ui(|ui| {
                                ui.label(format!("DNS: {}", dns_name));
                                for ip in ips {
                                    ui.label(format!("IP: {}", ip));
                                }
                            });
                        }
                        if let Some(sel) = new_selection {
                            self.selected_peer = Some(sel);
                        }
                    }
                });
            });

        // Received files
        egui::SidePanel::right("received_panel")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Received Files");
                ui.separator();

                // Active transfers
                if !self.transferring_files.is_empty() {
                    ui.label(RichText::new("Transferring...").strong());
                    for transfer in &self.transferring_files {
                        ui.horizontal(|ui| {
                            let progress = if transfer.size > 0 {
                                transfer.transferred as f32 / transfer.size as f32
                            } else {
                                0.0
                            };
                            ui.add(
                                egui::ProgressBar::new(progress)
                                    .text(&transfer.name)
                                    .desired_width(200.0),
                            );
                        });
                    }
                    ui.separator();
                }

                // Received files list
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.received_files.is_empty() {
                        ui.label("No files received yet");
                        ui.label(RichText::new("Files sent via Taildrop will appear here").weak());
                    } else {
                        let mut file_to_save = None;
                        let mut file_to_delete = None;

                        for (idx, file) in self.received_files.iter().enumerate() {
                            let is_selected = self.selected_received_file == Some(idx);

                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(is_selected, "📄").clicked() {
                                        self.selected_received_file = Some(idx);
                                    }
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(&file.name).strong());
                                        ui.label(
                                            RichText::new(format_size(file.size)).weak().small(),
                                        );
                                    });
                                });

                                if is_selected {
                                    ui.horizontal(|ui| {
                                        if ui.button("💾 Save As...").clicked() {
                                            file_to_save = Some(idx);
                                        }
                                        if ui.button("🗑 Delete").clicked() {
                                            file_to_delete = Some(idx);
                                        }
                                    });
                                }
                            });
                        }

                        // Handle save
                        if let Some(idx) = file_to_save {
                            if let Some(file) = self.received_files.get(idx) {
                                if let Some(dest) = rfd::FileDialog::new()
                                    .set_file_name(&file.name)
                                    .save_file()
                                {
                                    if std::fs::copy(&file.path, &dest).is_ok() {
                                        // Mark as saved or remove from list
                                        if let Some(f) = self.received_files.get_mut(idx) {
                                            f.saved = true;
                                        }
                                    }
                                }
                            }
                        }

                        // Handle delete
                        if let Some(idx) = file_to_delete {
                            if let Some(file) = self.received_files.get(idx) {
                                self.send_command(TailscaleCommand::DeleteReceivedFile(
                                    file.path.clone(),
                                ));
                            }
                            self.received_files.remove(idx);
                            self.selected_received_file = None;
                        }
                    }
                });
            });

        // File explorer
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Send Files");
            ui.separator();

            // Drop zone
            let drop_zone = ui.allocate_response(
                Vec2::new(ui.available_width(), 80.0),
                Sense::hover(),
            );

            let is_being_dragged = ctx.input(|i| !i.raw.hovered_files.is_empty());

            ui.painter().rect_filled(
                drop_zone.rect,
                8.0,
                if is_being_dragged {
                    Color32::from_rgba_unmultiplied(46, 204, 113, 30)
                } else {
                    Color32::from_rgba_unmultiplied(100, 100, 100, 20)
                },
            );

            ui.painter().rect_stroke(
                drop_zone.rect,
                8.0,
                egui::Stroke::new(
                    2.0,
                    if is_being_dragged {
                        Color32::from_rgb(46, 204, 113)
                    } else {
                        Color32::from_rgb(100, 100, 100)
                    },
                ),
                StrokeKind::Outside,
            );

            ui.scope_builder(
                egui::UiBuilder::new().max_rect(drop_zone.rect),
                |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("📁 Drop files here or browse below")
                                .size(16.0)
                                .color(Color32::GRAY),
                        );
                    });
                },
            );

            ui.add_space(8.0);

            // Files queued for sending
            if !self.files_to_send.is_empty() {
                ui.group(|ui| {
                    ui.label(RichText::new("Files to send:").strong());
                    let mut to_remove = None;
                    for (idx, path) in self.files_to_send.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "📰 {}",
                                path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default()
                            ));
                            if ui.small_button("🗙").clicked() {
                                to_remove = Some(idx);
                            }
                        });
                    }
                    if let Some(idx) = to_remove {
                        self.files_to_send.remove(idx);
                    }

                    ui.separator();

                    // Send button
                    let mut should_send = false;
                    let peer_hostname = self.selected_peer.as_ref().and_then(|pid| {
                        self.peers.iter().find(|p| &p.id == pid).map(|p| p.hostname.clone())
                    });

                    ui.horizontal(|ui| {
                        let can_send =
                            self.selected_peer.is_some() && !self.files_to_send.is_empty();

                        if ui
                            .add_enabled(can_send, egui::Button::new("💌 Send to Device"))
                            .clicked()
                        {
                            should_send = true;
                        }

                        if let Some(hostname) = &peer_hostname {
                            ui.label(format!("-> {}", hostname));
                        } else {
                            ui.label(RichText::new("Select a device first").weak());
                        }
                    });

                    if should_send {
                        if let Some(peer_id) = self.selected_peer.clone() {
                            let files: Vec<_> = self.files_to_send.drain(..).collect();
                            if let Some(tx) = &self.command_tx {
                                for file_path in files {
                                    let _ = tx.send(TailscaleCommand::SendFile {
                                        peer_id: peer_id.clone(),
                                        file_path,
                                    });
                                }
                            }
                        }
                    }
                });
            }

            ui.add_space(16.0);
            ui.separator();

            // File browser
            ui.heading("File Browser");

            // Navigation bar
            ui.horizontal(|ui| {
                if ui.button("⬆ Up").clicked() {
                    self.navigate_up();
                }
                if ui.button("🏠 Home").clicked() {
                    if let Ok(home) = std::env::var("HOME") {
                        self.navigate_to(PathBuf::from(home));
                    }
                }
                if ui.button("⟳").clicked() {
                    self.refresh_directory();
                }
                ui.separator();
                ui.label(self.current_directory.to_string_lossy());
            });

            ui.separator();

            // Directory contents
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut nav_to = None;
                let mut add_to_send = None;

                for (idx, entry) in self.directory_contents.iter().enumerate() {
                    let is_selected = self.selected_directory_item == Some(idx);
                    let icon = if entry.is_dir { "📂" } else { "📰" };

                    let response = ui.selectable_label(
                        is_selected,
                        format!(
                            "{} {}{}",
                            icon,
                            entry.name,
                            if entry.is_dir {
                                "/".to_string()
                            } else {
                                format!(" ({})", format_size(entry.size))
                            }
                        ),
                    );

                    if response.clicked() {
                        self.selected_directory_item = Some(idx);
                    }

                    if response.double_clicked() {
                        if entry.is_dir {
                            nav_to = Some(entry.path.clone());
                        } else {
                            add_to_send = Some(entry.path.clone());
                        }
                    }

                    // Context menu
                    response.context_menu(|ui| {
                        if !entry.is_dir {
                            if ui.button("Add to send queue").clicked() {
                                add_to_send = Some(entry.path.clone());
                                ui.close();
                            }
                        }
                        if entry.is_dir {
                            if ui.button("Open folder").clicked() {
                                nav_to = Some(entry.path.clone());
                                ui.close();
                            }
                        }
                    });
                }

                if let Some(path) = nav_to {
                    self.navigate_to(path);
                }

                if let Some(path) = add_to_send {
                    if !self.files_to_send.contains(&path) {
                        self.files_to_send.push(path);
                    }
                }
            });
        });
    }
}


fn format_size(bytes: u64) -> String {
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