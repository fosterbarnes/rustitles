//! GUI rendering components.

#[cfg(target_os = "linux")]
use crate::python_manager::PythonManager;
use crate::{
    config::APP_VERSION,
    data_structures::{JobStatus, SubtitleDownloader},
    debug,
    helper_functions::{Utils, Validation},
    info,
    settings::Settings,
    warn,
};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use rfd::FileDialog;

/// Donation and dependency text color.
const DEPS_YELLOW: egui::Color32 = egui::Color32::from_rgb(247, 233, 181);
/// Header subtitle color.
const TITLE_SUBTITLE: egui::Color32 = egui::Color32::from_rgb(0xAC, 0x98, 0xC7);

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn render_dependency_command(ui: &mut egui::Ui, label: &str, command: &str) -> bool {
    let mut copied_now = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let mut command_edit = command.to_string();
        ui.add(
            egui::TextEdit::singleline(&mut command_edit)
                .desired_width(350.0)
                .interactive(false)
                .font(egui::TextStyle::Monospace)
                .horizontal_align(egui::Align::Center),
        );
        if ui.button("Copy").clicked() {
            ui.output_mut(|output| {
                output
                    .commands
                    .push(egui::OutputCommand::CopyText(command.to_string()));
            });
            copied_now = true;
        }
        if copied_now {
            ui.label(egui::RichText::new("Copied!").color(egui::Color32::from_rgb(76, 175, 80)));
        }
    });
    copied_now
}

#[cfg(target_os = "linux")]
fn linux_install_command(apt: &str, dnf: &str, pacman: &str) -> String {
    match PythonManager::linux_package_manager() {
        "dnf" => dnf.to_string(),
        "pacman" => pacman.to_string(),
        _ => apt.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn macos_install_command(package: &str, homebrew_installed: bool) -> String {
    let shellenv = "eval \"$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv 2>/dev/null)\"";
    if homebrew_installed {
        return format!("{shellenv} && brew install {package}");
    }

    format!(
        "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\" && {shellenv} && brew install {package}"
    )
}

fn svg_icon_to_pos(x: f32, y: f32, icon_rect: egui::Rect) -> egui::Pos2 {
    const ORIGIN_X: f32 = 287.13005;
    const ORIGIN_Y: f32 = 220.77155;
    const SVG_W: f32 = 449.73325;
    const SVG_H: f32 = 449.72025;
    egui::pos2(
        icon_rect.min.x + (x - ORIGIN_X) / SVG_W * icon_rect.width(),
        icon_rect.min.y + (y - ORIGIN_Y) / SVG_H * icon_rect.height(),
    )
}

fn cubic_bezier(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let u = 1.0 - t;
    let uu = u * u;
    let uuu = uu * u;
    let tt = t * t;
    let ttt = tt * t;
    egui::pos2(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}

fn sample_cubic(
    out: &mut Vec<egui::Pos2>,
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    steps: u32,
) {
    let start = if out.is_empty() { 0 } else { 1 };
    for i in start..=steps {
        let t = i as f32 / steps as f32;
        out.push(cubic_bezier(p0, p1, p2, p3, t));
    }
}

/// Build the icon wipe curve.
fn icon_wipe_curve(icon_rect: egui::Rect, remaining: f32) -> Vec<egui::Pos2> {
    let shift = -remaining;
    let map = |x: f32, y: f32| {
        let p = svg_icon_to_pos(x, y, icon_rect);
        egui::pos2(p.x + shift, p.y)
    };
    let mut pts = Vec::with_capacity(48);
    let steps = 10;
    let tr0 = map(679.98, 220.79);
    let tr1 = map(687.15, 220.84);
    let tr2 = map(694.46, 220.35);
    let tr3 = map(701.43, 222.37);
    let tr4 = map(717.25, 226.43);
    let tr5 = map(730.40, 239.25);
    let tr6 = map(734.89, 254.94);
    let tr7 = map(737.47, 263.04);
    let tr8 = map(736.76, 271.62);
    let tr9 = map(736.81, 279.99);
    sample_cubic(&mut pts, tr0, tr1, tr2, tr3, steps);
    sample_cubic(&mut pts, tr3, tr4, tr5, tr6, steps);
    sample_cubic(&mut pts, tr6, tr7, tr8, tr9, steps);
    let br0 = map(736.77, 624.05);
    pts.push(br0);
    let br1 = map(736.73, 642.12);
    let br2 = map(725.00, 659.28);
    let br3 = map(708.49, 666.43);
    let br4 = map(702.35, 669.13);
    let br5 = map(695.63, 670.55);
    let br6 = map(688.91, 670.49);
    sample_cubic(&mut pts, br0, br1, br2, br3, steps);
    sample_cubic(&mut pts, br3, br4, br5, br6, steps);
    // Cover anti-aliased edge pixels.
    const AA_PAD: f32 = 2.0;
    if let Some(first) = pts.first().copied() {
        pts.insert(0, egui::pos2(first.x, icon_rect.min.y - AA_PAD));
    }
    if let Some(last) = pts.last().copied() {
        pts.push(egui::pos2(last.x, icon_rect.max.y + AA_PAD));
    }
    pts
}

fn fill_right_of_curve(
    painter: &egui::Painter,
    curve: &[egui::Pos2],
    right_x: f32,
    color: egui::Color32,
) {
    if curve.len() < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for pair in curve.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let ar = egui::pos2(right_x, a.y);
        let br = egui::pos2(right_x, b.y);
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, color);
        mesh.colored_vertex(ar, color);
        mesh.colored_vertex(br, color);
        mesh.colored_vertex(b, color);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

impl SubtitleDownloader {
    /// Render the application header.
    pub fn render_header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let github_url = "https://github.com/fosterbarnes/rustitles";
            let title_main = format!("Rustitles v{} ", APP_VERSION);
            let title_main_rich = egui::RichText::new(title_main)
                .color(egui::Color32::from_rgb(147, 115, 192))
                .size(17.0)
                .monospace()
                .strong();
            let title_sub_rich = egui::RichText::new("Subtitle Downloader Tool")
                .color(TITLE_SUBTITLE)
                .size(17.0)
                .monospace()
                .strong();
            let main_response = ui.add(
                egui::Label::new(title_main_rich)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            );
            let sub_response = ui.add(
                egui::Label::new(title_sub_rich)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            );
            if main_response.clicked() || sub_response.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(github_url));
            }
            if main_response.hovered() || sub_response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.dependencies_ready() {
                    let donation_url = "https://coff.ee/fosterbarnes";
                    let donation_text = "coff.ee/fosterbarnes";
                    let link_response = ui.hyperlink_to(
                        egui::RichText::new(donation_text)
                            .color(DEPS_YELLOW)
                            .monospace(),
                        donation_url,
                    );

                    if link_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
            });
        });
        ui.add_space(5.0);
    }

    /// Render the installation screen.
    pub fn render_installation_wait(&self, ui: &mut egui::Ui, fade_out: bool) {
        const ICON_BYTES: &[u8] = include_bytes!("../../.res/gui/loadingIcon.svg");
        const TEXT_BYTES: &[u8] = include_bytes!("../../.res/gui/loadingText.svg");
        let time = self.splash_started.elapsed().as_secs_f32();
        let icon_duration = 1.38_f32;
        let text_delay = 1.38_f32;
        let text_duration = 1.08_f32;
        let raw_icon = (time / icon_duration).clamp(0.0, 1.0);
        // Use a mild ease-out.
        let icon_progress = 1.0 - (1.0 - raw_icon).powf(1.5);
        let text_alpha = ((time - text_delay) / text_duration).clamp(0.0, 1.0);
        let out_alpha = if fade_out {
            self.splash_out_alpha()
        } else {
            1.0
        };

        if icon_progress < 1.0 || text_alpha < 1.0 || (fade_out && out_alpha > 0.001) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }

        let avail = ui.max_rect();
        let center = avail.center();
        let icon_size = egui::vec2(220.0, 220.0);
        let text_size = egui::vec2(320.0, 48.0);
        let gap = 32.0;
        let block_h = icon_size.y + gap + text_size.y;
        let block_top = center.y - block_h * 0.5 - 12.0;

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(center.x, block_top + icon_size.y * 0.5),
            icon_size,
        );
        let text_rect = egui::Rect::from_center_size(
            egui::pos2(center.x, block_top + icon_size.y + gap + text_size.y * 0.5),
            text_size,
        );

        let icon_image = egui::Image::from_bytes("bytes://loadingIcon.svg", ICON_BYTES)
            .fit_to_exact_size(icon_size)
            .tint(egui::Color32::from_white_alpha((out_alpha * 255.0) as u8));
        icon_image.paint_at(ui, icon_rect);

        if icon_progress < 1.0 {
            let remaining = icon_rect.width() * (1.0 - icon_progress);
            if remaining > 0.35 {
                let bg = ui.visuals().window_fill();
                let curve = icon_wipe_curve(icon_rect, remaining);
                fill_right_of_curve(ui.painter(), &curve, icon_rect.max.x + 2.0, bg);
            }
        }

        let alpha_u8 = (text_alpha * out_alpha * 255.0) as u8;
        let tint = egui::Color32::from_white_alpha(alpha_u8);
        let text_image = egui::Image::from_bytes("bytes://loadingText.svg", TEXT_BYTES)
            .fit_to_exact_size(text_size)
            .tint(tint);
        text_image.paint_at(ui, text_rect);

        if text_alpha * out_alpha > 0.05 {
            let credit_alpha = (text_alpha * out_alpha * 255.0) as u8;
            let color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, credit_alpha);
            let galley = ui.painter().layout_no_wrap(
                "made with <3 by fosterbarnes".to_string(),
                egui::TextStyle::Monospace.resolve(ui.style()),
                color,
            );
            let text_pos = egui::pos2(center.x - galley.size().x * 0.5, text_rect.max.y + 16.0);
            ui.painter().galley(text_pos, galley, color);
        }
    }

    /// Render Python installation status.
    pub fn render_python_status(&mut self, ui: &mut egui::Ui) {
        if self.is_python_installed() {
            let raw = self.get_python_version().cloned().unwrap_or_default();
            let ver = raw.strip_prefix("Python ").unwrap_or(&raw);
            ui.label(
                egui::RichText::new(format!("Python v{}", ver))
                    .color(DEPS_YELLOW)
                    .size(12.0)
                    .monospace(),
            );
        } else {
            ui.label(
                egui::RichText::new("Python not found")
                    .color(DEPS_YELLOW)
                    .size(12.0)
                    .monospace(),
            );
            #[cfg(windows)]
            if ui.button("Install Python").clicked() {
                info!("User initiated Python installation");
                self.start_python_install();
            }
            #[cfg(target_os = "linux")]
            {
                let command = linux_install_command(
                    "sudo apt install python3 python3-pip",
                    "sudo dnf install python3 python3-pip",
                    "sudo pacman -S python python-pip",
                );
                render_dependency_command(ui, "Install Python 3:", &command);
            }
            #[cfg(target_os = "macos")]
            {
                let command = macos_install_command("python", self.homebrew_installed);
                let label = if self.homebrew_installed {
                    "Install Python 3:"
                } else {
                    "Install Homebrew and Python 3:"
                };
                render_dependency_command(ui, label, &command);
            }
        }
    }

    /// Render pipx installation status.
    pub fn render_pipx_status(&mut self, _ui: &mut egui::Ui) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if self.is_python_installed() {
                if self.is_pipx_installed() {
                    let text = self
                        .get_pipx_version()
                        .map(|v| format!("pipx v{}", v))
                        .unwrap_or_else(|| "pipx is installed".to_string());
                    _ui.label(
                        egui::RichText::new(text)
                            .color(DEPS_YELLOW)
                            .size(12.0)
                            .monospace(),
                    );
                } else {
                    _ui.label(
                        egui::RichText::new("pipx not found")
                            .color(DEPS_YELLOW)
                            .size(12.0)
                            .monospace(),
                    );
                }
            }
        }
    }

    /// Render Subliminal installation status.
    pub fn render_subliminal_status(&mut self, ui: &mut egui::Ui) {
        if !self.subliminal_installed && self.subliminal_version.is_some() {
            ui.label(
                "Subliminal 2.4.0 or newer is required. Upgrade your Subliminal installation.",
            );
        }
        if self.is_python_installed() {
            #[cfg(target_os = "linux")]
            {
                if !self.is_pipx_installed() {
                    ui.label(
                        egui::RichText::new("Subliminal not found")
                            .color(DEPS_YELLOW)
                            .size(12.0)
                            .monospace(),
                    );
                    let command = linux_install_command(
                        "sudo apt install pipx && pipx install subliminal",
                        "sudo dnf install python3-pipx && pipx install subliminal",
                        "sudo pacman -S python-pipx && pipx install subliminal",
                    );
                    render_dependency_command(ui, "Install missing dependencies:", &command);
                    return;
                }
            }
            #[cfg(target_os = "macos")]
            {
                if !self.is_pipx_installed() && !self.is_subliminal_installed() {
                    ui.label(
                        egui::RichText::new("Subliminal not found")
                            .color(DEPS_YELLOW)
                            .size(12.0)
                            .monospace(),
                    );
                    let command = macos_install_command(
                        "pipx && pipx ensurepath && pipx install subliminal",
                        self.homebrew_installed,
                    );
                    let label = if self.homebrew_installed {
                        "Install pipx and Subliminal:"
                    } else {
                        "Install Homebrew, pipx, and Subliminal:"
                    };
                    render_dependency_command(ui, label, &command);
                    return;
                }
            }
            if self.is_subliminal_installed() && !self.installing_subliminal {
                let raw = self.get_subliminal_version().cloned().unwrap_or_default();
                let ver = raw
                    .strip_prefix("subliminal, version ")
                    .or_else(|| raw.strip_prefix("subliminal "))
                    .unwrap_or(&raw);
                ui.label(
                    egui::RichText::new(format!("Subliminal v{}", ver))
                        .color(DEPS_YELLOW)
                        .size(12.0)
                        .monospace(),
                );
            } else if !self.is_subliminal_installed() {
                ui.label(
                    egui::RichText::new("Subliminal not found")
                        .color(DEPS_YELLOW)
                        .size(12.0)
                        .monospace(),
                );
                #[cfg(windows)]
                if ui.button("Install Subliminal").clicked() {
                    info!("User initiated Subliminal installation");
                    self.start_subliminal_install();
                }
                #[cfg(target_os = "linux")]
                render_dependency_command(ui, "Install Subliminal:", "pipx install subliminal");
                #[cfg(target_os = "macos")]
                render_dependency_command(ui, "Install Subliminal:", "pipx install subliminal");
            }
        }
        if self.is_version_checked() {
            if let Some(latest) = self.get_latest_version() {
                if Self::is_outdated(APP_VERSION, latest) {
                    let exe_url = format!(
                        "https://github.com/fosterbarnes/rustitles/releases/tag/{}",
                        latest
                    );
                    let link_text = format!("-> Rustitles {}", latest);
                    let link_rich =
                        egui::RichText::new(link_text).color(egui::Color32::from_rgb(3, 169, 244));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "Your version is out of date. Download the latest release: ",
                            )
                            .color(egui::Color32::from_rgb(244, 67, 54)),
                        );
                        let resp = ui.hyperlink_to(link_rich, exe_url);
                        if resp.hovered() {
                            let painter = ui.painter();
                            let rect = resp.rect;
                            let y = rect.bottom() - 2.0;
                            painter.line_segment(
                                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                                egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(3, 169, 244)),
                            );
                        }
                    });
                }
            } else if let Some(err) = self.get_version_check_error() {
                ui.label(
                    egui::RichText::new(format!("Version check failed: {}", err))
                        .color(egui::Color32::from_rgb(230, 194, 0)),
                );
            }
        }
    }

    /// Render FFmpeg installation status.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn render_ffmpeg_status(&mut self, ui: &mut egui::Ui) {
        #[cfg(target_os = "linux")]
        if !self.is_ffmpeg_installed() {
            ui.label(
                egui::RichText::new("FFmpeg not found")
                    .color(DEPS_YELLOW)
                    .size(12.0)
                    .monospace(),
            );
            let command = linux_install_command(
                "sudo apt install ffmpeg",
                "sudo dnf install ffmpeg",
                "sudo pacman -S ffmpeg",
            );
            render_dependency_command(ui, "Install FFmpeg:", &command);
        }
        #[cfg(target_os = "macos")]
        if !self.is_ffmpeg_installed() {
            ui.label(
                egui::RichText::new("FFmpeg not found")
                    .color(DEPS_YELLOW)
                    .size(12.0)
                    .monospace(),
            );
            let command = macos_install_command("ffmpeg", self.homebrew_installed);
            let label = if self.homebrew_installed {
                "Install FFmpeg:"
            } else {
                "Install Homebrew and FFmpeg:"
            };
            render_dependency_command(ui, label, &command);
        }
    }

    /// Render language selection.
    pub fn render_language_selection(&mut self, ui: &mut egui::Ui) {
        const LANGUAGE_LIST: &[(&str, &str)] = &[
            ("en", "English"),
            ("en-gb", "English (UK)"),
            ("en-us", "English (US)"),
            ("af", "Afrikaans"),
            ("am", "Amharic"),
            ("ar", "Arabic"),
            ("az", "Azerbaijani"),
            ("bg", "Bulgarian"),
            ("bn", "Bengali"),
            ("cs", "Czech"),
            ("da", "Danish"),
            ("de", "German"),
            ("de-at", "German (Austria)"),
            ("de-ch", "German (Switzerland)"),
            ("el", "Greek"),
            ("es", "Spanish"),
            ("es-es", "Spanish (Spain)"),
            ("es-mx", "Spanish (Mexico)"),
            ("et", "Estonian"),
            ("fa", "Persian/Farsi"),
            ("fi", "Finnish"),
            ("fil", "Filipino/Tagalog"),
            ("fr", "French"),
            ("fr-ca", "French (Canada)"),
            ("gu", "Gujarati"),
            ("he", "Hebrew"),
            ("hi", "Hindi"),
            ("hr", "Croatian"),
            ("hu", "Hungarian"),
            ("id", "Indonesian"),
            ("is", "Icelandic"),
            ("it", "Italian"),
            ("it-ch", "Italian (Switzerland)"),
            ("ja", "Japanese"),
            ("ka", "Georgian"),
            ("km", "Khmer"),
            ("kn", "Kannada"),
            ("ko", "Korean"),
            ("ku", "Kurdish"),
            ("lo", "Lao"),
            ("lt", "Lithuanian"),
            ("lv", "Latvian"),
            ("ml", "Malayalam"),
            ("mn", "Mongolian"),
            ("ms", "Malay"),
            ("mt", "Maltese"),
            ("my", "Burmese"),
            ("nl", "Dutch"),
            ("nl-be", "Dutch (Belgium)"),
            ("no", "Norwegian"),
            ("or", "Odia"),
            ("pa", "Punjabi"),
            ("pl", "Polish"),
            ("pt", "Portuguese"),
            ("pt-br", "Portuguese (Brazil)"),
            ("pt-pt", "Portuguese (Portugal)"),
            ("ro", "Romanian"),
            ("ru", "Russian"),
            ("sk", "Slovak"),
            ("sl", "Slovenian"),
            ("sr", "Serbian"),
            ("sv", "Swedish"),
            ("sw", "Swahili"),
            ("ta", "Tamil"),
            ("te", "Telugu"),
            ("th", "Thai"),
            ("tr", "Turkish"),
            ("uk", "Ukrainian"),
            ("ur", "Urdu"),
            ("vi", "Vietnamese"),
            ("xh", "Xhosa"),
            ("zh", "Chinese"),
            ("zh-cn", "Chinese (Simplified)"),
            ("zh-tw", "Chinese (Traditional)"),
            ("zu", "Zulu"),
        ];

        ui.horizontal(|ui| {
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
                self.set_keep_dropdown_open(false);
                self.save_current_settings();
            }
            ui.add_space(0.0);
            let overwrite_existing = self.get_overwrite_existing_mut();
            let overwrite_checkbox_response = ui.checkbox(overwrite_existing, "Overwrite Existing Subtitles");
            if overwrite_checkbox_response.changed() {
                info!("(Overwrite Existing Subtitles) changed to: {}", *overwrite_existing);
                self.set_keep_dropdown_open(false);
                self.save_current_settings();
                if !self.get_folder_path().is_empty() {
                    self.scan_folder();
                }
            }

            let ignore_local_extras = self.get_ignore_local_extras_mut();
            let ignore_extras_checkbox_response = ui.checkbox(ignore_local_extras, "Ignore Extra Folders for Plex")
                .on_hover_ui(|ui| {
                    ui.set_width(300.0);
                    ui.label("Ignores 'Behind The Scenes', 'Deleted Scenes', 'Featurettes', 'Interviews', 'Scenes', 'Shorts', 'Trailers' and 'Other' folders (case-insensitive)");
                });
            if ignore_extras_checkbox_response.changed() {
                info!("(Ignore Local Extras) changed to: {}", *ignore_local_extras);
                self.set_keep_dropdown_open(false);
                self.save_current_settings();
                if !self.get_folder_path().is_empty() {
                    self.scan_folder();
                }
            }

            let skip_scanned_media = self.get_skip_scanned_media_mut();
            let skip_scanned_response = ui.checkbox(skip_scanned_media, "Skip scanned media")
                .on_hover_ui(|ui| {
                    ui.set_width(320.0);
                    ui.label("Skip files that already had a successful download unless the file changed or new languages are selected.");
                });
            if skip_scanned_response.changed() {
                info!("(Skip scanned media) changed to: {}", *self.get_skip_scanned_media_mut());
                self.set_keep_dropdown_open(false);
                self.save_current_settings();
                if !self.get_folder_path().is_empty() {
                    self.scan_folder();
                }
            }
        });

        if self.get_keep_dropdown_open() {
            ui.add_space(5.0);
            ui.group(|ui| {
                ui.set_width(200.0);

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for &(code, name) in LANGUAGE_LIST {
                            let selected_languages = self.get_selected_languages_mut();
                            let mut selected =
                                selected_languages.iter().any(|selected| selected == code);
                            let display_text = format!("{} [{}]", name, code);
                            if ui.checkbox(&mut selected, display_text).changed() {
                                if selected {
                                    selected_languages.push(code.to_string());
                                    debug!("Language selected: {}", code);
                                } else {
                                    selected_languages.retain(|c| c != code);
                                    debug!("Language deselected: {}", code);
                                }

                                self.save_current_settings();
                            }
                        }
                    });
            });
        }
    }

    /// Render matching options.
    pub fn render_matching_options(&mut self, ui: &mut egui::Ui) {
        const PROVIDERS: &[(&str, &str)] = &[
            ("addic7ed", "Addic7ed"),
            ("gestdown", "Gestdown"),
            ("napiprojekt", "NapiProjekt"),
            ("opensubtitles", "OpenSubtitles"),
            ("opensubtitlescom", "OpenSubtitles.com"),
            ("podnapisi", "Podnapisi"),
            ("tvsubtitles", "TVSubtitles"),
        ];
        const REFINERS: &[(&str, &str)] = &[
            ("hash", "Hash"),
            ("metadata", "Metadata"),
            ("tmdb", "TMDB"),
            ("tvdb", "TVDB"),
        ];

        let panel_response = egui::CollapsingHeader::new(egui::RichText::new("Subliminal Matching").color(egui::Color32::WHITE))
            .default_open(self.get_matching_options_open())
            .show(ui, |ui| {
             ui.label(egui::RichText::new("Defaults select all providers; clear every box to use Subliminal's full provider pool.").color(egui::Color32::WHITE).size(12.0));
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                for (code, name) in PROVIDERS {
                    let mut selected = self.providers.iter().any(|value| value == code);
                    if ui.checkbox(&mut selected, *name).changed() {
                        changed = true;
                        if selected {
                            if !self.providers.iter().any(|value| value == code) {
                                self.providers.push((*code).to_string());
                            }
                        } else {
                            self.providers.retain(|value| value != code);
                        }
                    }
                }
            });

            ui.label(egui::RichText::new("Refiners:").color(egui::Color32::WHITE));
            ui.horizontal_wrapped(|ui| {
                for (code, name) in REFINERS {
                    let mut selected = self.refiners.iter().any(|value| value == code);
                    if ui.checkbox(&mut selected, *name).changed() {
                        changed = true;
                        if selected {
                            if !self.refiners.iter().any(|value| value == code) {
                                self.refiners.push((*code).to_string());
                            }
                        } else {
                            self.refiners.retain(|value| value != code);
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Minimum match score:").color(egui::Color32::WHITE));
                let mut score_text = self.minimum_score.to_string();
                let response = ui.add_sized(
                    [35.0, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut score_text),
                );
                if response.changed() {
                    if let Ok(score) = score_text.parse::<u8>() {
                        if score <= 100 {
                            self.minimum_score = score;
                            changed = true;
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max OpenSubtitles.com pages:").color(egui::Color32::WHITE));
                let mut pages_text = self.opensubtitles_max_pages.map(|v| v.to_string()).unwrap_or_default();
                let response = ui.add_sized(
                    [35.0, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut pages_text),
                ).on_hover_text("Empty = unlimited. Default 3. Range 1-20.");
                if response.changed() {
                    let trimmed = pages_text.trim();
                    if trimmed.is_empty() {
                        self.opensubtitles_max_pages = None;
                        changed = true;
                    } else if let Ok(v) = trimmed.parse::<u8>() {
                        if (1..=20).contains(&v) {
                            self.opensubtitles_max_pages = Some(v);
                            changed = true;
                        }
                    }
                }
            });

            egui::CollapsingHeader::new(egui::RichText::new("OpenSubtitles.com credentials (optional, for higher limits)").color(egui::Color32::WHITE))
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("API key:").color(egui::Color32::WHITE));
                        let mut apikey = self.opensubtitlescom_apikey.clone();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut apikey)
                                .password(!self.show_opensubtitles_apikey)
                                .desired_width(260.0)
                                .hint_text("Paste from opensubtitles.com/api")
                        );
                        if resp.changed() {
                            self.opensubtitlescom_apikey = apikey;
                            changed = true;
                        }
                        if ui.button(if self.show_opensubtitles_apikey { "Hide" } else { "Show" }).clicked() {
                            self.show_opensubtitles_apikey = !self.show_opensubtitles_apikey;
                        }
                        let link = ui.hyperlink_to("Get API key", "https://www.opensubtitles.com/api");
                        if link.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Username:").color(egui::Color32::WHITE));
                        let mut username = self.opensubtitlescom_username.clone();
                        if ui.add(egui::TextEdit::singleline(&mut username).desired_width(140.0)).changed() {
                            self.opensubtitlescom_username = username;
                            changed = true;
                        }
                        ui.label(egui::RichText::new("Password:").color(egui::Color32::WHITE));
                        let mut password = self.opensubtitlescom_password.clone();
                        if ui.add(egui::TextEdit::singleline(&mut password).password(true).desired_width(140.0)).changed() {
                            self.opensubtitlescom_password = password;
                            changed = true;
                        }
                    });
                    ui.label(egui::RichText::new(Settings::credentials_storage_blurb()).color(egui::Color32::WHITE).weak().size(11.0));
                });

            if ui.checkbox(&mut self.exclude_hearing_impaired, "Exclude SDH / captions").changed() {
                changed = true;
            }

            if changed {
                self.save_current_settings();
            }
            });

        if panel_response.fully_open() && !self.get_matching_options_open() {
            self.set_matching_options_open(true);
            self.save_current_settings();
        } else if panel_response.fully_closed() && self.get_matching_options_open() {
            self.set_matching_options_open(false);
            self.save_current_settings();
        }
    }

    /// Render concurrent download settings.
    pub fn render_concurrent_downloads(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Concurrent Downloads:").color(egui::Color32::WHITE));
            let concurrent_downloads = self.get_concurrent_downloads_mut();
            let mut concurrent_text = concurrent_downloads.to_string();
            let text_response = ui.add_sized(
                [25.0, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut concurrent_text),
            );
            if text_response.changed() {
                if let Ok(value) = concurrent_text.parse::<usize>() {
                    if Validation::is_valid_concurrent_downloads(value) {
                        let old_value = *concurrent_downloads;
                        *concurrent_downloads = value;
                        debug!(
                            "Concurrent downloads changed from {} to {}",
                            old_value, concurrent_downloads
                        );
                        self.save_current_settings();
                    } else {
                        warn!("Invalid concurrent downloads value: {}", value);
                    }
                }
                self.set_keep_dropdown_open(false);
            }
            if text_response.gained_focus() {
                self.set_keep_dropdown_open(false);
            }
        });
    }

    /// Render folder selection.
    pub fn render_folder_selection(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Folder to scan:").color(egui::Color32::WHITE));
            let folder_button_response = ui.button("Select Folder");
            if folder_button_response.clicked() {
                self.set_keep_dropdown_open(false);
                if let Some(folder) = FileDialog::new().pick_folder() {
                    let new_folder = folder.display().to_string();
                    if self.get_folder_path() != new_folder
                        && Validation::is_valid_folder(&new_folder)
                    {
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
                    let (rect, _response) =
                        ui.allocate_exact_size(button_size, egui::Sense::hover());
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
                        painter.line_segment(
                            [p1, p2],
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(147, 115, 192)),
                        );
                    }
                } else {
                    let color = egui::Color32::from_rgb(136, 136, 136);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new("↻").color(color).size(16.0))
                            .frame(false),
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

    /// Render scan results.
    pub fn render_scan_results(&self, ui: &mut egui::Ui) {
        if !self.get_folder_path().is_empty() {
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

                if self.get_ignore_local_extras() && self.get_ignored_extra_folders() > 0 {
                    ui.add_space(5.0);
                    ui.label("-");
                    ui.add_space(5.0);
                    ui.label(format!(
                        "Ignoring {} extra folders",
                        self.get_ignored_extra_folders()
                    ));
                }
                if self.get_skipped_scanned_count() > 0 {
                    ui.add_space(5.0);
                    ui.label("-");
                    ui.add_space(5.0);
                    ui.label(format!(
                        "Skipping {} scanned",
                        self.get_skipped_scanned_count()
                    ));
                }
            });
        }
    }

    /// Render download job status.
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

        TableBuilder::new(ui)
            .max_scroll_height(available_height.max(100.0))
            .min_scrolled_height(100.0)
            .auto_shrink([false, false])
            .striped(true)
            .resizable(true)
            .vscroll(true)
            .column(Column::initial(260.0).at_least(150.0).resizable(true))
            .column(Column::auto().at_least(80.0).resizable(true))
            .column(Column::remainder().at_least(150.0))
            .header(row_height, |mut header| {
                header.col(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.strong("Video");
                    });
                });
                header.col(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.strong("Status");
                    });
                });
                header.col(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.strong("Output");
                    });
                });
            })
            .body(|mut body| {
                for job in cached_jobs {
                    let (status_text, status_color) = match &job.status {
                        JobStatus::Pending => ("Pending", egui::Color32::from_rgb(230, 194, 0)),
                        JobStatus::Running => ("Running", egui::Color32::from_rgb(147, 115, 192)),
                        JobStatus::Success => ("Success", egui::Color32::from_rgb(76, 175, 80)),
                        JobStatus::Skipped => ("Skipped", egui::Color32::from_rgb(136, 136, 136)),
                        JobStatus::EmbeddedExists(_) => {
                            ("Embedded", egui::Color32::from_rgb(230, 194, 0))
                        }
                        JobStatus::Failed(_) => ("Failed", egui::Color32::from_rgb(244, 67, 54)),
                    };

                    let subtitle_paths = &job.subtitle_paths;

                    body.row(row_height, |mut row| {
                        row.col(|ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let video_path = &job.video_path;
                                    let file_name = Utils::get_file_name(video_path);
                                    let scan_root = std::path::Path::new(&self.folder_path);
                                    let mut display_parts = Vec::new();
                                    if let Some(parent) =
                                        video_path.parent().filter(|path| *path != scan_root)
                                    {
                                        if let Some(show) = parent
                                            .parent()
                                            .filter(|path| *path != scan_root)
                                            .and_then(|path| path.file_name())
                                        {
                                            display_parts.push(show.to_string_lossy().into_owned());
                                        }
                                        if let Some(season) = parent.file_name() {
                                            display_parts
                                                .push(season.to_string_lossy().into_owned());
                                        }
                                    }
                                    display_parts.push(file_name);
                                    let display_name =
                                        display_parts.join(std::path::MAIN_SEPARATOR_STR);
                                    let path_str = video_path.display().to_string();
                                    let response = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&display_name).monospace(),
                                        )
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                    );
                                    if response.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if response.clicked() {
                                        if let Err(e) = Utils::open_containing_folder(video_path) {
                                            warn!("Failed to open folder for {}: {}", path_str, e);
                                        }
                                    }
                                    response.on_hover_text(&path_str);
                                },
                            );
                        });

                        row.col(|ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(status_text)
                                            .color(status_color)
                                            .monospace(),
                                    );
                                },
                            );
                        });

                        row.col(|ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| match &job.status {
                                    JobStatus::Success => {
                                        if let Some(sub_path) = subtitle_paths.first() {
                                            let path_str = sub_path.display().to_string();
                                            let color = egui::Color32::from_rgb(76, 175, 80);
                                            let response = ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&path_str)
                                                        .color(color)
                                                        .monospace(),
                                                )
                                                .sense(egui::Sense::click()),
                                            );
                                            if response.hovered() {
                                                ui.ctx().set_cursor_icon(
                                                    egui::CursorIcon::PointingHand,
                                                );
                                            }
                                            if response.clicked() {
                                                if let Err(e) =
                                                    Utils::open_containing_folder(sub_path)
                                                {
                                                    warn!(
                                                        "Failed to open folder for {}: {}",
                                                        path_str, e
                                                    );
                                                }
                                            }
                                            response
                                                .on_hover_text_at_pointer("Open containing folder");
                                        }
                                    }
                                    JobStatus::Failed(err) => {
                                        let color = egui::Color32::from_rgb(244, 67, 54);
                                        if err.contains("see log") {
                                            let response = ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(err)
                                                        .color(color)
                                                        .monospace(),
                                                )
                                                .sense(egui::Sense::click()),
                                            );
                                            if response.hovered() {
                                                ui.ctx().set_cursor_icon(
                                                    egui::CursorIcon::PointingHand,
                                                );
                                            }
                                            if response.clicked() {
                                                if let Err(e) = Utils::open_log_file() {
                                                    warn!("Failed to open log file: {}", e);
                                                }
                                            }
                                            response.on_hover_text_at_pointer("Open log file");
                                        } else {
                                            ui.label(
                                                egui::RichText::new(err).color(color).monospace(),
                                            );
                                        }
                                    }
                                    JobStatus::EmbeddedExists(msg) => {
                                        ui.label(
                                            egui::RichText::new(msg)
                                                .color(egui::Color32::from_rgb(230, 194, 0))
                                                .monospace(),
                                        );
                                    }
                                    JobStatus::Skipped => {
                                        ui.label(
                                            egui::RichText::new(&job.output)
                                                .color(egui::Color32::from_rgb(136, 136, 136))
                                                .monospace(),
                                        );
                                    }
                                    JobStatus::Pending | JobStatus::Running => {
                                        if !job.output.is_empty() {
                                            let color = egui::Color32::from_rgb(172, 152, 199);
                                            let response = ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&job.output)
                                                        .color(color)
                                                        .monospace(),
                                                )
                                                .truncate(),
                                            );
                                            response.on_hover_text_at_pointer(&job.output);
                                        } else if let Some(sub_path) = subtitle_paths.first() {
                                            let srt_name = sub_path
                                                .file_name()
                                                .map(|name| name.to_string_lossy().to_string())
                                                .unwrap_or_else(|| sub_path.display().to_string());
                                            ui.label(
                                                egui::RichText::new(&srt_name)
                                                    .color(egui::Color32::from_rgb(172, 152, 199))
                                                    .monospace(),
                                            );
                                        }
                                    }
                                },
                            );
                        });
                    });
                }
            });
    }

    /// Render download status.
    pub fn render_status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.is_downloading() {
                let time = ui.ctx().input(|i| i.time) as f32;
                let rotation_speed = 2.0;
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

                    painter.line_segment(
                        [p1, p2],
                        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(147, 115, 192)),
                    );
                }

                ui.add_space(20.0);
            } else if self.get_total_downloads() > 0
                && self.get_downloads_completed() == self.get_total_downloads()
            {
                let cached_jobs = self.get_cached_jobs();
                let all_failed = cached_jobs
                    .iter()
                    .all(|j| matches!(j.status, JobStatus::Failed(_)));
                let all_succeeded = cached_jobs.iter().all(|j| {
                    matches!(
                        j.status,
                        JobStatus::Success | JobStatus::EmbeddedExists(_) | JobStatus::Skipped
                    )
                });

                let center = ui.cursor().min + egui::vec2(8.0, 8.0);
                let painter = ui.painter();
                let stroke_width: f32 = 2.0;

                if all_failed {
                    let x_color = egui::Color32::from_rgb(244, 67, 54);

                    let p1 = center + egui::vec2(-4.0, -4.0);
                    let p2 = center + egui::vec2(4.0, 4.0);
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, x_color));

                    let p3 = center + egui::vec2(4.0, -4.0);
                    let p4 = center + egui::vec2(-4.0, 4.0);
                    painter.line_segment([p3, p4], egui::Stroke::new(stroke_width, x_color));
                } else if all_succeeded {
                    let check_color = egui::Color32::from_rgb(76, 175, 80);

                    let p1 = center + egui::vec2(-4.0, 0.0);
                    let p2 = center + egui::vec2(-1.0, 3.0);
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, check_color));

                    let p3 = center + egui::vec2(-1.0, 3.0);
                    let p4 = center + egui::vec2(4.0, -2.0);
                    painter.line_segment([p3, p4], egui::Stroke::new(stroke_width, check_color));
                }
                ui.add_space(20.0);
            }

            ui.label(&self.status);
        });
    }

    /// Render the progress bar.
    pub fn render_progress_bar(&self, ui: &mut egui::Ui) {
        let completed_count = self.get_downloads_completed();
        let total = self.get_total_downloads();
        if total > 0 {
            ui.add_space(10.0);
            let progress_text = format!(
                "Progress: {} / {} ({})",
                completed_count,
                total,
                Utils::format_progress(completed_count, total)
            );
            ui.label(progress_text);
        }
        if total > 0 {
            let progress = completed_count as f32 / total as f32;
            let window_width = ui.available_width();
            let progress_bar = egui::ProgressBar::new(progress)
                .show_percentage()
                .fill(egui::Color32::from_rgb(118, 94, 152))
                .corner_radius(egui::CornerRadius::same(3))
                .desired_width(window_width - 18.0);
            ui.add(progress_bar);
        }
    }
}

impl eframe::App for SubtitleDownloader {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.scroll.floating = false;
            style.spacing.scroll.bar_width = 8.0;
        });

        self.poll_init_check();
        self.handle_installation_states();
        self.begin_splash_dismiss_if_ready();
        if !self.can_show_main_ui() {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.45);
                    self.render_installation_wait(ui, true);
                });
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            return;
        }

        self.check_download_completion();
        self.refresh_installation_status();
        self.poll_version_check();

        let has_jobs = !self.get_cached_jobs().is_empty();
        let has_folder = !self.folder_path.is_empty();
        if has_jobs || has_folder {
            egui::Panel::bottom("status_panel").show(ui, |ui| {
                ui.add_space(8.0);
                self.render_status(ui);
                self.render_progress_bar(ui);
                ui.add_space(8.0);
            });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            self.render_header(ui);

            if self.installing_python || self.installing_subliminal {
                self.render_installation_wait(ui, false);
                return;
            }

            self.render_python_status(ui);
            self.render_pipx_status(ui);
            self.render_subliminal_status(ui);
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            self.render_ffmpeg_status(ui);
            ui.separator();

            if self.dependencies_ready() {
                self.render_language_selection(ui);
                ui.separator();
                self.render_matching_options(ui);
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

        if self.scanning {
            if let Some(rx) = &self.scan_done_receiver {
                if let Ok((ignored_count, skipped_count, scan_settings)) = rx.try_recv() {
                    self.scanning = false;
                    self.status = "Scan completed.".to_string();
                    self.scan_done_receiver = None;

                    self.ignored_extra_folders = ignored_count;
                    self.skipped_scanned_count = skipped_count;
                    if ignored_count > 0 {
                        info!(
                            "Scan completed with {} extra folders ignored",
                            ignored_count
                        );
                    }
                    if skipped_count > 0 {
                        info!(
                            "Scan completed with {} previously scanned files marked skipped",
                            skipped_count
                        );
                    }

                    info!("Scan completed, starting downloads automatically");
                    self.start_downloads_with_settings(scan_settings);
                }
            }
        }

        if self.downloading || self.scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
        if self.installing_python || self.installing_subliminal {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self) {
        self.prepare_for_exit();

        self.background_check_receiver = None;

        if let Some(handle) = self.background_check_handle.take() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = tx.send(());
            });

            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(_) => {
                    info!("Background thread exited gracefully");
                }
                Err(_) => {
                    warn!(
                        "Background thread did not exit within timeout, continuing with shutdown"
                    );
                }
            }
        }

        info!("Application closed by user");
        info!("");
        info!("---------------------------------------------------------------");
        info!("");
    }
}
