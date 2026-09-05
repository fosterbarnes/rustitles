//! Rustitles subtitle downloader library.

pub mod app;
pub mod config;
pub mod data_structures;
pub mod gui;
pub mod helper_functions;
pub mod logging;
pub mod python_manager;
pub mod scan_history;
pub mod settings;
pub mod subtitle_utils;

// Re-export shared items
pub use config::*;
pub use data_structures::*;
pub use helper_functions::*;
pub use logging::*;
pub use python_manager::*;
pub use settings::*;
pub use subtitle_utils::*;
