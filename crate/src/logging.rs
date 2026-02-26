//! Asynchronous logging system for the Rustitles application
//! 
//! This module provides a non-blocking logging system that writes log messages
//! to files without impacting the main application performance.

use std::io::Write;
use std::sync::Mutex;
use std::collections::VecDeque;
use std::sync::mpsc;

/// Asynchronous logger that writes to file without blocking the main thread
pub struct AsyncLogger {
    sender: mpsc::Sender<LogMessage>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// Types of log messages that can be sent to the logger
#[derive(Clone)]
pub enum LogMessage {
    Info(String),
    Warn(String),
    Error(String),
    Debug(String),
    Shutdown,
}

impl AsyncLogger {
    /// Create a new async logger that writes to a log file
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        
        // Get the log file path based on platform
        let log_path = {
            #[cfg(windows)]
            {
                let exe_path = std::env::current_exe()?;
                let exe_dir = exe_path.parent().ok_or("Failed to get executable directory")?;
                exe_dir.join("rustitles_log.txt")
            }
            
            #[cfg(not(windows))]
            {
                // Use XDG cache directory on Linux
                if let Ok(xdg_dirs) = xdg::BaseDirectories::new() {
                    let cache_dir = xdg_dirs.get_cache_home();
                    let app_dir = cache_dir.join("rustitles");
                    std::fs::create_dir_all(&app_dir)?;
                    app_dir.join("rustitles.log")
                } else {
                    // Fallback to home directory
                    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
                    let app_dir = home_dir.join(".rustitles");
                    std::fs::create_dir_all(&app_dir)?;
                    app_dir.join("rustitles.log")
                }
            }
        };
        
        // Create or open the log file
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        
        let handle = std::thread::spawn(move || {
            let mut file = std::io::BufWriter::new(log_file);
            let mut buffer = VecDeque::new();
            
            loop {
                // Process messages in batches for better performance
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        LogMessage::Shutdown => {
                            // Flush any remaining messages
                            for entry in buffer.drain(..) {
                                let _ = writeln!(file, "{}", entry);
                            }
                            let _ = file.flush();
                            return;
                        }
                        _ => {
                            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                            let entry = match msg {
                                LogMessage::Info(msg) => format!("[INFO {} src\\main.rs:0] {}", timestamp, msg),
                                LogMessage::Warn(msg) => format!("[WARN {} src\\main.rs:0] {}", timestamp, msg),
                                LogMessage::Error(msg) => format!("[ERROR {} src\\main.rs:0] {}", timestamp, msg),
                                LogMessage::Debug(msg) => format!("[DEBUG {} src\\main.rs:0] {}", timestamp, msg),
                                LogMessage::Shutdown => unreachable!(),
                            };
                            buffer.push_back(entry);
                        }
                    }
                }
                
                // Flush buffer if it has enough entries or if we've been idle
                if buffer.len() >= 10 {
                    for entry in buffer.drain(..) {
                        let _ = writeln!(file, "{}", entry);
                    }
                    let _ = file.flush();
                }
                
                // Small sleep to prevent busy waiting
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        
        Ok(AsyncLogger {
            sender: tx,
            handle: Some(handle),
        })
    }
    
    /// Send a log message to the async logger
    pub fn log(&self, level: &str, message: &str) {
        let msg = match level {
            "INFO" => LogMessage::Info(message.to_string()),
            "WARN" => LogMessage::Warn(message.to_string()),
            "ERROR" => LogMessage::Error(message.to_string()),
            "DEBUG" => LogMessage::Debug(message.to_string()),
            _ => LogMessage::Info(message.to_string()),
        };
        
        // Non-blocking send - if the channel is full, we just drop the message
        let _ = self.sender.send(msg);
    }
    
    /// Gracefully shutdown the logger
    pub fn shutdown(self) {
        let _ = self.sender.send(LogMessage::Shutdown);
        if let Some(handle) = self.handle {
            let _ = handle.join();
        }
    }
}

// Global logger instance
pub(crate) static LOGGER: Mutex<Option<AsyncLogger>> = Mutex::new(None);

/// Initialize the global logging system
pub fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    let logger = AsyncLogger::new()?;
    let mut guard = LOGGER.lock().map_err(|e| format!("Failed to lock logger: {}", e))?;
    *guard = Some(logger);
    Ok(())
}

/// Send a message to the global logger
pub fn log_message(level: &str, message: &str) {
    if let Ok(guard) = LOGGER.lock() {
        if let Some(logger) = &*guard {
            logger.log(level, message);
        }
    }
}

// Custom log macros
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::logging::log_message("INFO", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::logging::log_message("WARN", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::logging::log_message("ERROR", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::logging::log_message("DEBUG", &format!($($arg)*));
    };
} 