//! GUI rendering components for the Rustitles subtitle downloader
//! 
//! This module contains all the UI rendering methods and components.

use eframe::egui;
use egui_extras::{TableBuilder, Column};
use rfd::FileDialog;
use crate::{
    config::APP_VERSION,
    data_structures::{SubtitleDownloader, JobStatus},
    helper_functions::{Utils, Validation},
    info, warn, debug,
};

impl SubtitleDownloader {
    /// Render the application header
    pub fn render_header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Title on the left as a clickable link
            let title = format!("Rustitles v{} - Subtitle Downloader Tool", APP_VERSION);
            let github_url = "https://github.com/fosterbarnes/rustitles";
            let title_response = ui.hyperlink_to(
                egui::RichText::new(title).color(egui::Color32::from_rgb(189, 147, 249)).heading(),
                github_url
            );
            
            // Set cursor icon on hover
            if title_response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            
            // Add space to push donation link to the right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Only show donation link when both Python and Subliminal are installed
                if self.is_python_installed() && self.is_subliminal_installed() {
                    let donation_url = "https://buymeacoffee.com/fosterbarnes";
                    let donation_text = "buymeacoffee.com/fosterbarnes";
                    let link_response = ui.hyperlink_to(
                        egui::RichText::new(donation_text).color(egui::Color32::from_hex("#54b2fa").unwrap()),
                        donation_url
                    );
                    
                    // Set cursor icon on hover
                    if link_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
            });
        });
        ui.add_space(5.0);
    }

    /// Render installation wait screen
    pub fn render_installation_wait(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Draw spinner
            let time = ui.ctx().input(|i| i.time) as f32;
            let rotation_speed = 2.0; // radians per second, matches download spinner
            let angle = (time * rotation_speed) % (2.0 * std::f32::consts::PI);
            let center = ui.cursor().min + egui::vec2(8.0, 8.0);
            let radius = 6.0;
            let painter = ui.painter();
            let start_angle = angle;
            let end_angle = angle + std::f32::consts::PI * 1.5;
            let segments = 16;
            let angle_step = (end_angle - start_angle) / segments as f32;
            for i in 0..segments {
                let angle1 = start_angle + i as f32 * angle_step;
                let angle2 = start_angle + (i + 1) as f32 * angle_step;
                let p1 = center + egui::vec2(radius * angle1.cos(), radius * angle1.sin());
                let p2 = center + egui::vec2(radius * angle2.cos(), radius * angle2.sin());
                painter.line_segment([p1, p2], egui::Stroke::new(2.0, egui::Color32::from_rgb(189, 147, 249)));
            }
            ui.add_space(16.0);
            // Show status
            ui.label(self.get_status());
        });
    }

    /// Render Python installation status
    pub fn render_python_status(&mut self, ui: &mut egui::Ui) {
        if self.is_python_installed() {
            let raw = self.get_python_version().cloned().unwrap_or_default();
            let ver = raw.strip_prefix("Python ").unwrap_or(&raw);
            ui.label(format!("Python v{}", ver));
        } else {
            ui.label("Python not found");
            #[cfg(windows)]
            if ui.button("Install Python").clicked() {
                info!("User initiated Python installation");
                self.start_python_install();
            }
            #[cfg(target_os = "linux")]
            {
                ui.label("Please install Python 3 and python3-pip using your package manager, then restart Rustitles.");
            }
            #[cfg(target_os = "macos")]
            {
                ui.label("Please install Python 3. You can download it from python.org or use Homebrew: 'brew install python3'");
            }
        }
    }

    /// Render pipx installation status (Linux only)
    pub fn render_pipx_status(&mut self, _ui: &mut egui::Ui) {
        #[cfg(target_os = "linux")]
        {
            if self.is_python_installed() {
                if self.is_pipx_installed() {
                    let text = self.get_pipx_version()
                        .map(|v| format!("pipx v{}", v))
                        .unwrap_or_else(|| "pipx is installed".to_string());
                    _ui.label(text);
                } else {
                    _ui.label("pipx not found");
                }
            }
        }
    }

    /// Render Subliminal installation status
    pub fn render_subliminal_status(&mut self, ui: &mut egui::Ui) {
        if self.is_python_installed() {
            #[cfg(target_os = "linux")]
            {
                if !self.is_pipx_installed() {
                    ui.label("Subliminal not found");
                    ui.horizontal(|ui| {
                        ui.label("Install missing dependencies:");
                        let cmd = "sudo apt install pipx && pipx install subliminal".to_string();
                        let mut cmd_edit = cmd.clone();
                        ui.add(egui::TextEdit::singleline(&mut cmd_edit)
                            .desired_width(350.0)
                            .interactive(false)
                            .font(egui::TextStyle::Monospace)
                            .horizontal_align(egui::Align::Center));
                        let copy_icon = egui::RichText::new("Copy").size(14.0);
                        if ui.add(egui::Button::new(copy_icon)).on_hover_text("Copy to clipboard").clicked() {
                            ui.output_mut(|o| o.copied_text = cmd.clone());
                            self.set_pipx_copied(true);
                            self.set_pipx_copy_time(Some(std::time::Instant::now()));
                        }
                        if self.is_pipx_copied() {
                            ui.label(egui::RichText::new("Copied!").color(egui::Color32::from_rgb(80, 250, 123)));
                        }
                    });
                    return;
                }
            }
            if self.is_subliminal_installed() && !self.installing_subliminal {
                let raw = self.get_subliminal_version().cloned().unwrap_or_default();
                let ver = raw.strip_prefix("subliminal, version ")
                    .or_else(|| raw.strip_prefix("subliminal "))
                    .unwrap_or(&raw);
                ui.label(format!("Subliminal v{}", ver));
            } else if !self.is_subliminal_installed() {
                ui.label("Subliminal not found");
                if ui.button("Install Subliminal").clicked() {
                    info!("User initiated Subliminal installation");
                    // Note: This would need to be handled in the app logic
                    // For now, we'll just set the flag and let the app handle it
                }
            }
        }
        // Version check warning
        if self.is_version_checked() {
            if let Some(latest) = self.get_latest_version() {
                if Self::is_outdated(APP_VERSION, latest) {
                    let exe_url = format!("https://github.com/fosterbarnes/rustitles/releases/tag/{}", latest);
                    let link_text = format!("-> Rustitles {}", latest);
                    let link_rich = egui::RichText::new(link_text).color(egui::Color32::from_rgb(80, 160, 255));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("Your version is out of date. Download the latest release: ").color(egui::Color32::from_rgb(255, 85, 85)));
                        let resp = ui.hyperlink_to(link_rich, exe_url);
                        if resp.hovered() {
                            let painter = ui.painter();
                            let rect = resp.rect;
                            let y = rect.bottom() - 2.0;
                            painter.line_segment([
                                egui::pos2(rect.left(), y),
                                egui::pos2(rect.right(), y)
                            ], egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 160, 255)));
                        }
                    });
                }
            } else if let Some(err) = self.get_version_check_error() {
                ui.label(egui::RichText::new(format!("Version check failed: {}", err)).color(egui::Color32::from_rgb(255, 184, 108)));
            }
        }
    }

    /// Render language selection interface
    pub fn render_language_selection(&mut self, ui: &mut egui::Ui) {
        let language_list = vec![
            // English and variants at the top
            ("en", "English"), ("en-gb", "English (UK)"), ("en-us", "English (US)"),
            
            // All other languages sorted alphabetically
            ("af", "Afrikaans"), ("am", "Amharic"), ("ar", "Arabic"), ("az", "Azerbaijani"),
            ("bg", "Bulgarian"), ("bn", "Bengali"), ("cs", "Czech"), ("da", "Danish"),
            ("de", "German"), ("de-at", "German (Austria)"), ("de-ch", "German (Switzerland)"),
            ("el", "Greek"), ("es", "Spanish"), ("es-es", "Spanish (Spain)"), ("es-mx", "Spanish (Mexico)"),
            ("et", "Estonian"), ("fa", "Persian/Farsi"), ("fi", "Finnish"), ("fil", "Filipino/Tagalog"),
            ("fr", "French"), ("fr-ca", "French (Canada)"), ("gu", "Gujarati"), ("he", "Hebrew"),
            ("hi", "Hindi"), ("hr", "Croatian"), ("hu", "Hungarian"), ("id", "Indonesian"),
            ("is", "Icelandic"), ("it", "Italian"), ("it-ch", "Italian (Switzerland)"), ("ja", "Japanese"),
            ("ka", "Georgian"), ("km", "Khmer"), ("kn", "Kannada"), ("ko", "Korean"),
            ("ku", "Kurdish"), ("lo", "Lao"), ("lt", "Lithuanian"), ("lv", "Latvian"),
            ("ml", "Malayalam"), ("mn", "Mongolian"), ("ms", "Malay"), ("mt", "Maltese"),
            ("my", "Burmese"), ("nl", "Dutch"), ("nl-be", "Dutch (Belgium)"), ("no", "Norwegian"),
            ("or", "Odia"), ("pa", "Punjabi"), ("pl", "Polish"), ("pt", "Portuguese"),
            ("pt-br", "Portuguese (Brazil)"), ("pt-pt", "Portuguese (Portugal)"), ("ro", "Romanian"),
            ("ru", "Russian"), ("sk", "Slovak"), ("sl", "Slovenian"), ("sv", "Swedish"),
            ("sw", "Swahili"), ("ta", "Tamil"), ("te", "Telugu"), ("th", "Thai"),
            ("tr", "Turkish"), ("uk", "Ukrainian"), ("ur", "Urdu"), ("vi", "Vietnamese"),
            ("xh", "Xhosa"), ("zh", "Chinese"), ("zh-cn", "Chinese (Simplified)"), ("zh-tw", "Chinese (Traditional)"),
            ("zu", "Zulu")
        ];

        ui.horizontal(|ui| {
            // Button that looks like ComboBox (no dropdown arrow)
            let selected_languages = self.get_selected_languages_mut();
            let selected_text = if selected_languages.is_empty() {
                "Select Languages".to_string()
            } else {
                selected_languages.join(", ")
            };
            
            let button_response = ui.add_sized([130.0, ui.spacing().interact_size.y], egui::Button::new(selected_text));
            if button_response.clicked() {
                debug!("Button clicked! Current state: {}", self.get_keep_dropdown_open());
                self.set_keep_dropdown_open(!self.get_keep_dropdown_open());
                debug!("New state: {}", self.get_keep_dropdown_open());
            }

            let force_download = self.get_force_download_mut();
            let force_checkbox_response = ui.checkbox(force_download, "Ignore Embedded Subtitles");
            if force_checkbox_response.changed() {
                info!("(Ignore Embedded Subtitles) changed to: {}", *force_download);
                self.set_keep_dropdown_open(false); // Close dropdown when checkbox is clicked
                self.save_current_settings(); // Save settings when changed
            }
            ui.add_space(0.0);
            let overwrite_existing = self.get_overwrite_existing_mut();
            let overwrite_checkbox_response = ui.checkbox(overwrite_existing, "Overwrite Existing Subtitles");
            if overwrite_checkbox_response.changed() {
                info!("(Overwrite Existing Subtitles) changed to: {}", *overwrite_existing);
                self.set_keep_dropdown_open(false); // Close dropdown when checkbox is clicked
                self.save_current_settings(); // Save settings when changed
                // Re-scan for missing subtitles when overwrite option changes
                if !self.get_folder_path().is_empty() {
                    self.scan_folder();
                }
            }
            
            let ignore_local_extras = self.get_ignore_local_extras_mut();
            let ignore_extras_checkbox_response = ui.checkbox(ignore_local_extras, "Ignore Extra Folders for Plex")
                .on_hover_ui(|ui| {
                    ui.set_width(300.0);
                    ui.label("Ignores 'Behind The Scenes', 'Deleted Scenes', 'Featurettes', 'Interviews', 'Scenes', 'Shorts', 'Trailers' and 'Other' folders");
                });
            if ignore_extras_checkbox_response.changed() {
                info!("(Ignore Local Extras) changed to: {}", *ignore_local_extras);
                self.set_keep_dropdown_open(false); // Close dropdown when checkbox is clicked
                self.save_current_settings(); // Save settings when changed
                // Re-scan for missing subtitles when ignore extras option changes
                if !self.get_folder_path().is_empty() {
                    self.scan_folder();
                }
            }
        });
        
        // Simple popup that shows when button is clicked
        if self.get_keep_dropdown_open() {
            ui.add_space(5.0);
            ui.group(|ui| {
                ui.set_width(200.0);
                
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width()); // Make scrollbar flush right
                        for (code, name) in &language_list {
                            let selected_languages = self.get_selected_languages_mut();
                            let mut selected = selected_languages.contains(&code.to_string());
                            let display_text = format!("{} [{}]", name, code);
                            if ui.checkbox(&mut selected, display_text).changed() {
                                if selected {
                                    selected_languages.push(code.to_string());
                                    debug!("Language selected: {}", code);
                                } else {
                                    selected_languages.retain(|c| c != code);
                                    debug!("Language deselected: {}", code);
                                }
                                
                                self.save_current_settings(); // Save settings when languages change
                            }
                        }
                    });
            });
        }
    }

    /// Render concurrent downloads setting
    pub fn render_concurrent_downloads(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Concurrent Downloads:");
            let concurrent_downloads = self.get_concurrent_downloads_mut();
            let mut concurrent_text = concurrent_downloads.to_string();
            let text_response = ui.add_sized([25.0, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut concurrent_text));
            if text_response.changed() {
                if let Ok(value) = concurrent_text.parse::<usize>() {
                    if Validation::is_valid_concurrent_downloads(value) {
                        let old_value = *concurrent_downloads;
                        *concurrent_downloads = value;
                        debug!("Concurrent downloads changed from {} to {}", old_value, concurrent_downloads);
                        self.save_current_settings(); // Save settings when changed
                    } else {
                        warn!("Invalid concurrent downloads value: {}", value);
                    }
                }
                self.set_keep_dropdown_open(false); // Close dropdown when text field is changed
            }
            if text_response.gained_focus() {
                self.set_keep_dropdown_open(false); // Close dropdown when text field gains focus
            }
        });
    }

    /// Render folder selection interface
    pub fn render_folder_selection(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Folder to scan:");
            let folder_button_response = ui.button("Select Folder");
            if folder_button_response.clicked() {
                self.set_keep_dropdown_open(false);
                if let Some(folder) = FileDialog::new().pick_folder() {
                    let new_folder = folder.display().to_string();
                    if self.get_folder_path() != new_folder && Validation::is_valid_folder(&new_folder) {
                        info!("Folder selected: {}", new_folder);
                        self.set_folder_path(new_folder);
                        self.scan_folder();
                    } else if !Validation::is_valid_folder(&new_folder) {
                        warn!("Invalid folder selected: {}", new_folder);
                    }
                }
            }
            ui.label(self.get_folder_path());

            if !self.get_folder_path().is_empty() {
                let scanning = self.is_scanning();
                if scanning {
                    let button_size = egui::vec2(20.0, 20.0);
                    let (rect, _response) = ui.allocate_exact_size(button_size, egui::Sense::hover());
                    let painter = ui.painter();
                    let center = rect.center();
                    let radius = 7.0;
                    let time = ui.ctx().input(|i| i.time) as f32;
                    let angle = (time * 2.0) % (2.0 * std::f32::consts::PI);
                    let start_angle = angle;
                    let end_angle = angle + std::f32::consts::PI * 1.5;
                    let segments = 16;
                    let angle_step = (end_angle - start_angle) / segments as f32;
                    for i in 0..segments {
                        let a1 = start_angle + i as f32 * angle_step;
                        let a2 = start_angle + (i + 1) as f32 * angle_step;
                        let p1 = center + egui::vec2(radius * a1.cos(), radius * a1.sin());
                        let p2 = center + egui::vec2(radius * a2.cos(), radius * a2.sin());
                        painter.line_segment([p1, p2], egui::Stroke::new(2.0, egui::Color32::from_rgb(189, 147, 249)));
                    }
                } else {
                    let color = egui::Color32::from_rgb(160, 160, 160);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new("↻").color(color).size(16.0))
                            .frame(false)
                    );
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        info!("Rescan button clicked");
                        self.set_keep_dropdown_open(false);
                        self.scan_folder();
                    }
                    response.on_hover_text("Rescan folder");
                }
            }
        });
    }

    /// Render scan results summary
    pub fn render_scan_results(&self, ui: &mut egui::Ui) {
        if !self.get_folder_path().is_empty() {
            // Take quick snapshots to minimize lock time
            let scanned_count = {
                if let Ok(videos) = self.scanned_videos.lock() {
                    videos.len()
                } else {
                    0
                }
            };
            let missing_count = {
                if let Ok(videos) = self.videos_missing_subs.lock() {
                    videos.len()
                } else {
                    0
                }
            };
            ui.horizontal(|ui| {
                ui.label(format!("Found videos: {}", scanned_count));
                ui.add_space(5.0);
                ui.label("-");
                ui.add_space(5.0);
                if self.get_overwrite_existing() {
                    ui.label(format!("Overwriting {} subtitles", missing_count));
                } else {
                    ui.label(format!("Missing subtitles: {}", missing_count));
                }
                
                // Show ignored extra folders count if the feature is enabled and folders were ignored
                if self.get_ignore_local_extras() && self.get_ignored_extra_folders() > 0 {
                    ui.add_space(5.0);
                    ui.label("-");
                    ui.add_space(5.0);
                    ui.label(format!("Ignoring {} extra folders", self.get_ignored_extra_folders()));
                }
            });
        }
    }

    /// Render download jobs status as a data grid
    pub fn render_download_jobs(&mut self, ui: &mut egui::Ui) {
        self.update_cached_jobs();

        let cached_jobs = self.get_cached_jobs();
        if cached_jobs.is_empty() {
            return;
        }

        ui.label("Subliminal Jobs:");
        ui.separator();

        let row_height = 22.0;
        let available_height = ui.available_height();

        egui::ScrollArea::vertical()
            .max_height(available_height.max(100.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .vscroll(false)
                    .column(Column::initial(120.0).at_least(80.0).resizable(true))
                    .column(Column::auto().at_least(80.0).resizable(true))
                    .column(Column::remainder().at_least(150.0))
                    .header(row_height, |mut header| {
                        header.col(|ui| { ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.strong("Video"); }); });
                        header.col(|ui| { ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.strong("Status"); }); });
                        header.col(|ui| { ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.strong("Output"); }); });
                    })
                    .body(|mut body| {
                        for job in cached_jobs {
                            let (status_text, status_color) = match &job.status {
                                JobStatus::Pending => ("Pending".to_string(), egui::Color32::from_rgb(241, 250, 140)),
                                JobStatus::Running => ("Running".to_string(), egui::Color32::from_rgb(189, 147, 249)),
                                JobStatus::Success => ("Success".to_string(), egui::Color32::from_rgb(80, 250, 123)),
                                JobStatus::EmbeddedExists(_) => ("Embedded".to_string(), egui::Color32::from_rgb(255, 184, 108)),
                                JobStatus::Failed(_) => ("Failed".to_string(), egui::Color32::from_rgb(255, 85, 85)),
                            };

                            let subtitle_paths = job.subtitle_paths.clone();

                            body.row(row_height, |mut row| {
                                row.col(|ui| {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let file_name = Utils::get_file_name(&job.video_path);
                                        let video_path = job.video_path.clone();
                                        let path_str = video_path.display().to_string();
                                        let response = ui.add(
                                            egui::Label::new(&file_name).truncate(true).sense(egui::Sense::click())
                                        );
                                        if response.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        if response.clicked() {
                                            if let Err(e) = Utils::open_containing_folder(&video_path) {
                                                warn!("Failed to open folder for {}: {}", path_str, e);
                                            }
                                        }
                                        response.on_hover_text(&path_str);
                                    });
                                });

                                row.col(|ui| {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(&status_text).color(status_color));
                                    });
                                });

                                row.col(|ui| {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        match &job.status {
                                            JobStatus::Success => {
                                                if let Some(sub_path) = subtitle_paths.first() {
                                                    let sub_path_clone = sub_path.clone();
                                                    let path_str = sub_path.display().to_string();
                                                    let color = egui::Color32::from_rgb(80, 250, 123);
                                                    let response = ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&path_str).color(color)
                                                        ).sense(egui::Sense::click())
                                                    );
                                                    if response.hovered() {
                                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                    }
                                                    if response.clicked() {
                                                        if let Err(e) = Utils::open_containing_folder(&sub_path_clone) {
                                                            warn!("Failed to open folder for {}: {}", path_str, e);
                                                        }
                                                    }
                                                    response.on_hover_text("Open containing folder");
                                                }
                                            }
                                            JobStatus::Failed(err) => {
                                                let color = egui::Color32::from_rgb(255, 85, 85);
                                                if err.contains("see log") {
                                                    let response = ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(err).color(color)
                                                        ).sense(egui::Sense::click())
                                                    );
                                                    if response.hovered() {
                                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                    }
                                                    if response.clicked() {
                                                        if let Err(e) = Utils::open_log_file() {
                                                            warn!("Failed to open log file: {}", e);
                                                        }
                                                    }
                                                    response.on_hover_text("Open log file");
                                                } else {
                                                    ui.label(egui::RichText::new(err).color(color));
                                                }
                                            }
                                            JobStatus::EmbeddedExists(msg) => {
                                                ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(255, 184, 108)));
                                            }
                                            JobStatus::Pending | JobStatus::Running => {
                                                if let Some(sub_path) = subtitle_paths.first() {
                                                    let srt_name = sub_path.file_name()
                                                        .map(|n| n.to_string_lossy().to_string())
                                                        .unwrap_or_else(|| sub_path.display().to_string());
                                                    ui.label(egui::RichText::new(&srt_name).color(egui::Color32::from_rgb(184, 146, 239)));
                                                }
                                            }
                                        }
                                    });
                                });
                            });
                        }
                    });
            });
    }

    /// Render status with optional spinning indicator or check mark
    pub fn render_status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Show spinning indicator when downloading, check mark when complete
            if self.is_downloading() {
                let time = ui.ctx().input(|i| i.time) as f32;
                // Use a constant rotation speed (2 radians per second) for smooth animation
                let rotation_speed = 2.0; // radians per second
                let angle = (time * rotation_speed) % (2.0 * std::f32::consts::PI);
                
                // Draw spinning circle
                let center = ui.cursor().min + egui::vec2(8.0, 8.0);
                let radius = 6.0;
                let painter = ui.painter();
                
                // Draw the spinning arc using circle segments
                let start_angle = angle;
                let end_angle = angle + std::f32::consts::PI * 1.5; // 3/4 of a circle
                
                // Draw arc using multiple line segments
                let segments = 16;
                let angle_step = (end_angle - start_angle) / segments as f32;
                
                for i in 0..segments {
                    let angle1 = start_angle + i as f32 * angle_step;
                    let angle2 = start_angle + (i + 1) as f32 * angle_step;
                    
                    let p1 = center + egui::vec2(
                        radius * angle1.cos(),
                        radius * angle1.sin()
                    );
                    let p2 = center + egui::vec2(
                        radius * angle2.cos(),
                        radius * angle2.sin()
                    );
                    
                    painter.line_segment(
                        [p1, p2],
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(189, 147, 249))
                    );
                }
                
                ui.add_space(20.0); // Space between spinner and text
            } else if self.get_total_downloads() > 0 && self.get_downloads_completed() == self.get_total_downloads() {
                // Check if all downloads failed or all succeeded
                let cached_jobs = self.get_cached_jobs();
                let all_failed = cached_jobs.iter().all(|j| {
                    matches!(j.status, JobStatus::Failed(_))
                });
                let all_succeeded = cached_jobs.iter().all(|j| {
                    matches!(j.status, JobStatus::Success | JobStatus::EmbeddedExists(_))
                });
                
                let center = ui.cursor().min + egui::vec2(8.0, 8.0);
                let painter = ui.painter();
                let stroke_width = 2.0;
                
                if all_failed {
                    // Show red X when all downloads failed
                    let x_color = egui::Color32::from_rgb(255, 85, 85); // Red color
                    
                    // First line of X (top-left to bottom-right)
                    let p1 = center + egui::vec2(-4.0, -4.0);
                    let p2 = center + egui::vec2(4.0, 4.0);
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, x_color));
                    
                    // Second line of X (top-right to bottom-left)
                    let p3 = center + egui::vec2(4.0, -4.0);
                    let p4 = center + egui::vec2(-4.0, 4.0);
                    painter.line_segment([p3, p4], egui::Stroke::new(stroke_width, x_color));
                } else if all_succeeded {
                    // Show check mark when all downloads succeeded
                    let check_color = egui::Color32::from_rgb(80, 250, 123); // Green color
                    
                    // First line of check mark (top-left to middle)
                    let p1 = center + egui::vec2(-4.0, 0.0);
                    let p2 = center + egui::vec2(-1.0, 3.0);
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, check_color));
                    
                    // Second line of check mark (middle to bottom-right)
                    let p3 = center + egui::vec2(-1.0, 3.0);
                    let p4 = center + egui::vec2(4.0, -2.0);
                    painter.line_segment([p3, p4], egui::Stroke::new(stroke_width, check_color));
                }
                // If mixed results (some succeeded, some failed), show no icon
                
                ui.add_space(20.0); // Space between icon and text
            }
            
            ui.label(&self.status);
        });
    }

    /// Render progress bar
    pub fn render_progress_bar(&self, ui: &mut egui::Ui) {
        // Count all jobs that are not Pending or Running as completed
        let cached_jobs = self.get_cached_jobs();
        let completed_count = cached_jobs.iter().filter(|j| {
            !matches!(j.status, JobStatus::Pending | JobStatus::Running)
        }).count();
        let total = self.get_total_downloads();
        // Show progress bar only when downloads are active or complete
        if self.is_downloading() || (!self.is_downloading() && total > 0) {
            if total > 0 {
                ui.add_space(10.0);
                let progress_text = format!("Progress: {} / {} ({})", 
                    completed_count, 
                    total,
                    Utils::format_progress(completed_count, total)
                );
                ui.label(progress_text);
            }
        }
        // Place the progress bar here, outside the ScrollArea. always fit the window
        if (self.is_downloading() || (!self.is_downloading() && total > 0)) && total > 0 {
            let progress = completed_count as f32 / total as f32;
            let window_width = ui.ctx().screen_rect().width();
            let progress_bar = egui::ProgressBar::new(progress)
                .show_percentage()
                .fill(egui::Color32::from_rgb(124, 99, 160)) // #7c63a0
                .desired_width(window_width - 18.0);
            ui.add(progress_bar);
        }
    }
}

impl eframe::App for SubtitleDownloader {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Scroll bar: fixed size, no expand-on-hover
        ctx.style_mut(|style| {
            style.spacing.scroll.floating = false;
            style.spacing.scroll.bar_width = 8.0;
        });

        self.poll_init_check();
        self.check_download_completion();
        self.refresh_installation_status();
        self.handle_installation_states();
        self.poll_version_check();

        let has_jobs = !self.get_cached_jobs().is_empty();
        let has_folder = !self.folder_path.is_empty();
        if has_jobs || has_folder {
            egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
                ui.add_space(8.0);
                self.render_status(ui);
                self.render_progress_bar(ui);
                ui.add_space(8.0);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_header(ui);

            if self.checking_deps || self.installing_python || self.installing_subliminal {
                self.render_installation_wait(ui);
                return;
            }

            self.render_python_status(ui);
            self.render_pipx_status(ui);
            self.render_subliminal_status(ui);
            ui.separator();

            if self.subliminal_installed {
                self.render_language_selection(ui);
                ui.separator();
                self.render_concurrent_downloads(ui);
                ui.separator();
                self.render_folder_selection(ui);
                ui.separator();
                self.render_scan_results(ui);
                self.render_download_jobs(ui);
            } else {
                ui.label("Please install all dependencies before downloading subtitles.");
            }
        });

        // When scan finishes, start downloads automatically
        if self.scanning {
            if let Some(rx) = &self.scan_done_receiver {
                if let Ok(ignored_count) = rx.try_recv() {
                    self.scanning = false;
                    self.status = "Scan completed.".to_string();
                    self.scan_done_receiver = None;
                    
                    // Update the ignored extra folders count
                    self.ignored_extra_folders = ignored_count;
                    if ignored_count > 0 {
                        info!("Scan completed with {} extra folders ignored", ignored_count);
                    }

                    // Start downloads automatically after scan
                    info!("Scan completed, starting downloads automatically");
                    self.start_downloads();
                }
            }
        }

        if self.downloading || self.scanning {
            // ~60 FPS for smooth spinner animation during downloads or scanning
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
        // Reset pipx_copied after 1.5 seconds
        if self.pipx_copied {
            if let Some(t) = self.pipx_copy_time {
                if t.elapsed().as_secs_f32() > 1.5 {
                    self.pipx_copied = false;
                    self.pipx_copy_time = None;
                }
            }
        }

        if self.checking_deps || self.installing_python || self.installing_subliminal {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Signal all background threads to stop via the atomic flag
        self.shutdown_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        // Drop the receiver so the background thread's send() also fails immediately
        self.background_check_receiver = None;

        if let Some(handle) = self.background_check_handle.take() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = tx.send(());
            });

            // 500ms is plenty -- the thread checks the flag every 100ms
            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(_) => {
                    info!("Background thread exited gracefully");
                }
                Err(_) => {
                    warn!("Background thread did not exit within timeout, continuing with shutdown");
                }
            }
        }
        
        info!("Application closed by user");
        info!("");
        info!("---------------------------------------------------------------");
        info!("");
    }
} 