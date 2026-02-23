//! Rustitles - Subtitle Downloader Tool
//! 
//! A desktop application for automatically downloading subtitles for video files.
//! Built with Rust and egui for cross-platform (Windows, Linux & macOS)

// Import all modules
mod config;
mod data_structures;
mod logging;
mod settings;
mod python_manager;
mod subtitle_utils;
mod app;
mod gui;
mod helper_functions;

// Re-export commonly used items
pub use config::*;
pub use data_structures::*;
pub use logging::*;
pub use settings::*;
pub use python_manager::*;
pub use subtitle_utils::*;
pub use helper_functions::*;

// Only keep actually used imports
use image;
use serde_json;
use crate::logging::LOGGER;

// Third-party crate imports
use eframe::egui;

// Platform-specific imports
#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Initialize the application with logging and configuration
fn initialize_app() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    if let Err(e) = setup_logging() {
        eprintln!("Failed to initialize logging: {}", e);
    }
    
    info!("Starting Rustitles application");
    Ok(())
}

/// Load application icon from embedded resources
fn load_app_icon() -> Option<egui::IconData> {
    #[cfg(windows)]
    {
        if let Ok(image) = image::load_from_memory(include_bytes!("../resources/rustitles_icon.ico")) {
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
    
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // Try PNG first on Linux/macOS, then fallback to ICO
        if let Ok(image) = image::load_from_memory(include_bytes!("../resources/rustitles_icon.png")) {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as u32, rgba.height() as u32];
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: size[0],
                height: size[1],
            })
        } else if let Ok(image) = image::load_from_memory(include_bytes!("../resources/rustitles_icon.ico")) {
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

/// Calculate window position to center on the currently used monitor (Windows only)
#[cfg(windows)]
fn calculate_window_position(window_size: [f32; 2]) -> egui::Pos2 {
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
                egui::Pos2::new(x, y)
            } else {
                egui::Pos2::new(100.0, 100.0)
            }
        } else {
            egui::Pos2::new(100.0, 100.0)
        }
    }
}

/// Configure the application window and visuals
fn configure_window(icon_data: Option<egui::IconData>) -> eframe::NativeOptions {
    let window_size = WINDOW_SIZE;

    #[cfg(windows)]
    let viewport_base = {
        let center_pos = calculate_window_position(window_size);
        egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_position(center_pos)
    };

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let viewport_base = egui::ViewportBuilder::default()
        .with_inner_size(window_size)
        .with_centered();

    let mut viewport_builder = viewport_base
        .with_decorations(true)
        .with_resizable(true)
        .with_min_inner_size(MIN_WINDOW_SIZE);
    
    if let Some(icon) = icon_data {
        viewport_builder = viewport_builder.with_icon(icon);
    }

    eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    }
}

/// Apply Dracula theme
fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    
    // Dracula theme accent colors
    visuals.override_text_color = Some(egui::Color32::from_rgb(248, 248, 242)); // #f8f8f2 (light gray)
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(189, 147, 249); // #bd93f9 (purple)
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(139, 233, 253); // #8be9fd (cyan)
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(68, 71, 90); // #44475a (darker gray)
    visuals.selection.bg_fill = egui::Color32::from_rgb(189, 147, 249); // #bd93f9 (purple)
    visuals.hyperlink_color = egui::Color32::from_rgb(139, 233, 253); // #8be9fd (cyan)
    visuals.warn_fg_color = egui::Color32::from_rgb(255, 184, 108); // #ffb86c (orange)
    visuals.error_fg_color = egui::Color32::from_rgb(255, 85, 85); // #ff5555 (red)
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(68, 71, 90); // #44475a
    visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(248, 248, 242); // #f8f8f2 (white text on purple)
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(40, 42, 54); // #282a36 (dark text on cyan)
    
    ctx.set_visuals(visuals);
}

/// Cleanup resources when the application exits
fn cleanup_on_exit() {
    // Shutdown logger when app exits
    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(logger) = guard.take() {
            logger.shutdown();
        }
    }
}

// =============================================================================
// ERROR HANDLING
// =============================================================================

/// Custom error type for application-specific errors
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    

}

// =============================================================================
// VERSION CONSTANT & VERSION CHECK STATE
// =============================================================================

fn main() {
    // Initialize the application
    if let Err(e) = initialize_app() {
        eprintln!("Failed to initialize application: {}", e);
        return;
    }
    
    // Load application icon
    let icon_data = load_app_icon();
    
    // Configure window
    let native_options = configure_window(icon_data);
    
    info!("Initializing GUI with window size: {}x{}", WINDOW_SIZE[0], WINDOW_SIZE[1]);
    
    // Run the application
    let result = eframe::run_native(
        "Rustitles",
        native_options,
        Box::new(|cc| {
            // Configure visuals
            configure_visuals(&cc.egui_ctx);
            
            info!("GUI initialized successfully");
            Box::new(SubtitleDownloader::default())
        }),
    );
    
    // Cleanup on exit
    cleanup_on_exit();
    
    result.expect("Failed to start eframe");
}