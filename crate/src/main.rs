//! Rustitles subtitle downloader.

// Application modules
mod app;
mod config;
mod data_structures;
mod gui;
mod helper_functions;
mod logging;
mod python_manager;
mod scan_history;
mod settings;
mod subtitle_utils;

// Re-export shared items
pub use config::*;
pub use data_structures::*;
pub use helper_functions::*;
pub use logging::*;
pub use python_manager::*;
pub use settings::*;
pub use subtitle_utils::*;

// Logging
use crate::logging::LOGGER;

// Third-party imports
use eframe::egui;

// Platform imports
#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Initialize the application.
fn initialize_app() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging.
    if let Err(e) = setup_logging() {
        eprintln!("Failed to initialize logging: {}", e);
    }

    info!("Starting Rustitles application");
    Ok(())
}

/// Load the application icon.
fn load_app_icon() -> Option<egui::IconData> {
    #[cfg(windows)]
    {
        if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_icon.ico"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width(), rgba.height()];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else {
            warn!("Failed to load application icon");
            None
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Use the macOS icon.
        if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_mac_icon.png"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_icon.png"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_icon.ico"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else {
            warn!("Failed to load application icon");
            None
        }
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        // Try PNG, then ICO.
        if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_icon.png"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else if let Ok(image) =
            image::load_from_memory(include_bytes!("../resources/rustitles_icon.ico"))
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else {
            warn!("Failed to load application icon");
            None
        }
    }
}

/// Center the window on the active monitor.
fn calculate_window_position(window_size: [f32; 2]) -> egui::Pos2 {
    #[cfg(not(windows))]
    let _ = window_size;

    #[cfg(windows)]
    {
        unsafe {
            let mut point = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut point).is_ok() {
                let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info).as_bool() {
                    let work_left = info.rcWork.left;
                    let work_top = info.rcWork.top;
                    let work_width = (info.rcWork.right - info.rcWork.left) as f32;
                    let work_height = (info.rcWork.bottom - info.rcWork.top) as f32;
                    let x = work_left as f32 + (work_width - window_size[0]) / 2.0;
                    let y = work_top as f32 + (work_height - window_size[1]) / 2.0;
                    return egui::Pos2::new(x, y);
                }
            }
        }
    }

    egui::Pos2::new(100.0, 100.0)
}

/// Configure the application window.
fn configure_window(icon_data: Option<egui::IconData>) -> eframe::NativeOptions {
    let window_size = WINDOW_SIZE;
    let initial_position = calculate_window_position(window_size);

    let mut viewport_builder = egui::ViewportBuilder::default()
        .with_inner_size(window_size)
        .with_position(initial_position)
        .with_decorations(true)
        .with_resizable(true)
        .with_min_inner_size(MIN_WINDOW_SIZE); // Minimum window size.

    if let Some(icon) = icon_data {
        viewport_builder = viewport_builder.with_icon(icon);
    }

    eframe::NativeOptions {
        viewport: viewport_builder,
        persist_window: true,
        ..Default::default()
    }
}

/// Apply the theme.
fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(egui::Color32::from_rgb(255, 255, 255));
    visuals.window_fill = egui::Color32::from_rgb(27, 27, 27);
    visuals.panel_fill = egui::Color32::from_rgb(27, 27, 27);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(82, 62, 110);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(64, 64, 64);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 45);
    visuals.selection.bg_fill = egui::Color32::from_rgb(77, 60, 100);
    visuals.hyperlink_color = egui::Color32::from_rgb(147, 115, 192);
    visuals.warn_fg_color = egui::Color32::from_rgb(230, 194, 0);
    visuals.error_fg_color = egui::Color32::from_rgb(244, 67, 54);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(45, 45, 45);
    visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(255, 255, 255);

    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
}

/// Configure the embedded font.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../resources/fonts/Inter-Regular.ttf"
        ))),
    );
    if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        proportional.insert(0, "Inter".to_owned());
    }
    if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        monospace.push("Inter".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Cleanup.
fn cleanup_on_exit() {
    // Shutdown logger.
    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(logger) = guard.take() {
            logger.shutdown();
        }
    }
}

fn main() {
    // Initialize the app.
    if let Err(e) = initialize_app() {
        eprintln!("Failed to initialize application: {}", e);
        return;
    }

    // Load icon.
    let icon_data = load_app_icon();

    // Configure window.
    let native_options = configure_window(icon_data);

    info!(
        "Initializing GUI with window size: {}x{}",
        WINDOW_SIZE[0], WINDOW_SIZE[1]
    );

    // Run the application.
    let result = eframe::run_native(
        "Rustitles",
        native_options,
        Box::new(|cc| {
            // Configure visuals.
            configure_visuals(&cc.egui_ctx);
            configure_fonts(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            info!("GUI initialized successfully");
            Ok(Box::new(SubtitleDownloader::default()))
        }),
    );

    cleanup_on_exit();

    if let Err(error) = result {
        eprintln!("Failed to start eframe: {error}");
    }
}
