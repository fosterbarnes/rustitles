//! Rustitles - Subtitle Downloader Library
//! 
//! This library provides the core functionality for downloading subtitles
//! for video files using the Subliminal Python package.

pub mod config;
pub mod data_structures;
pub mod logging;
pub mod settings;
pub mod python_manager;
pub mod subtitle_utils;
pub mod app;
pub mod gui;
pub mod helper_functions;

// Re-export commonly used items
pub use config::*;
pub use data_structures::*;
pub use logging::*;
pub use settings::*;
pub use python_manager::*;
pub use subtitle_utils::*;
pub use helper_functions::*; 