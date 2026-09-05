//! Asynchronous application logging.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(100);

fn log_wait(buffer_len: usize, elapsed: Duration) -> Duration {
    if buffer_len == 0 {
        LOG_FLUSH_INTERVAL
    } else if buffer_len >= 10 {
        Duration::ZERO
    } else {
        LOG_FLUSH_INTERVAL.saturating_sub(elapsed)
    }
}

fn append_log_message(buffer: &mut VecDeque<String>, msg: LogMessage) -> bool {
    match msg {
        LogMessage::Shutdown => true,
        _ => {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let entry = match msg {
                LogMessage::Info(msg) => format!("[INFO {}] {}", timestamp, msg),
                LogMessage::Warn(msg) => format!("[WARN {}] {}", timestamp, msg),
                LogMessage::Error(msg) => format!("[ERROR {}] {}", timestamp, msg),
                LogMessage::Debug(msg) => format!("[DEBUG {}] {}", timestamp, msg),
                LogMessage::Shutdown => String::new(),
            };
            buffer.push_back(entry);
            false
        }
    }
}

fn flush_log_buffer(
    file: &mut impl Write,
    buffer: &mut VecDeque<String>,
    last_flush: &mut Instant,
) -> io::Result<()> {
    for entry in buffer.drain(..) {
        writeln!(file, "{}", entry)?;
    }
    file.flush()?;
    *last_flush = Instant::now();
    Ok(())
}

fn report_logger_failure(reported: &AtomicBool, error: impl std::fmt::Display) {
    if !reported.swap(true, Ordering::Relaxed) {
        eprintln!("Rustitles logger error: {}", error);
    }
}

/// Logger that writes without blocking the main thread.
pub struct AsyncLogger {
    sender: mpsc::Sender<LogMessage>,
    handle: Option<std::thread::JoinHandle<()>>,
    failure_reported: Arc<AtomicBool>,
}

/// Log message types.
#[derive(Clone)]
pub enum LogMessage {
    Info(String),
    Warn(String),
    Error(String),
    Debug(String),
    Shutdown,
}

impl AsyncLogger {
    /// Create a logger.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();

        // Get the platform log path.
        let log_path = {
            #[cfg(windows)]
            {
                let exe_path = std::env::current_exe()?;
                let exe_dir = exe_path
                    .parent()
                    .ok_or("Failed to get executable directory")?;
                exe_dir.join("rustitles_log.txt")
            }

            #[cfg(not(windows))]
            {
                // Use the XDG cache directory.
                let xdg_dirs = xdg::BaseDirectories::new();
                if let Some(cache_dir) = xdg_dirs.get_cache_home() {
                    let app_dir = cache_dir.join("rustitles");
                    std::fs::create_dir_all(&app_dir)?;
                    app_dir.join("rustitles.log")
                } else {
                    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
                    let app_dir = home_dir.join(".rustitles");
                    std::fs::create_dir_all(&app_dir)?;
                    app_dir.join("rustitles.log")
                }
            }
        };

        // Open the log file.
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let failure_reported = Arc::new(AtomicBool::new(false));
        let thread_failure_reported = Arc::clone(&failure_reported);
        let handle = std::thread::spawn(move || {
            let mut file = std::io::BufWriter::new(log_file);
            let mut buffer = VecDeque::new();
            let mut last_flush = Instant::now();
            let failure_reported = thread_failure_reported;

            loop {
                let wait = log_wait(buffer.len(), last_flush.elapsed());
                match rx.recv_timeout(wait) {
                    Ok(msg) => {
                        if append_log_message(&mut buffer, msg) {
                            if let Err(error) =
                                flush_log_buffer(&mut file, &mut buffer, &mut last_flush)
                            {
                                report_logger_failure(&failure_reported, error);
                            }
                            return;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Err(error) =
                            flush_log_buffer(&mut file, &mut buffer, &mut last_flush)
                        {
                            report_logger_failure(&failure_reported, error);
                        }
                        return;
                    }
                }

                while let Ok(msg) = rx.try_recv() {
                    if append_log_message(&mut buffer, msg) {
                        if let Err(error) =
                            flush_log_buffer(&mut file, &mut buffer, &mut last_flush)
                        {
                            report_logger_failure(&failure_reported, error);
                        }
                        return;
                    }
                }

                // Flush full or old buffers.
                if buffer.len() >= 10
                    || (!buffer.is_empty() && last_flush.elapsed() >= LOG_FLUSH_INTERVAL)
                {
                    if let Err(error) = flush_log_buffer(&mut file, &mut buffer, &mut last_flush) {
                        report_logger_failure(&failure_reported, error);
                        return;
                    }
                }
            }
        });

        Ok(AsyncLogger {
            sender: tx,
            handle: Some(handle),
            failure_reported,
        })
    }

    /// Send a log message.
    pub fn log(&self, level: &str, message: &str) {
        let msg = match level {
            "INFO" => LogMessage::Info(message.to_string()),
            "WARN" => LogMessage::Warn(message.to_string()),
            "ERROR" => LogMessage::Error(message.to_string()),
            "DEBUG" => LogMessage::Debug(message.to_string()),
            _ => LogMessage::Info(message.to_string()),
        };

        // Keep GUI logging non-blocking.
        if let Err(error) = self.sender.send(msg) {
            report_logger_failure(&self.failure_reported, error);
        }
    }

    /// Shut down the logger.
    pub fn shutdown(self) {
        if let Err(error) = self.sender.send(LogMessage::Shutdown) {
            report_logger_failure(&self.failure_reported, error);
        }
        if let Some(handle) = self.handle {
            let _ = handle.join();
        }
    }
}

// Global logger
pub(crate) static LOGGER: Mutex<Option<AsyncLogger>> = Mutex::new(None);

/// Initialize global logging.
pub fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    let previous = {
        let mut guard = LOGGER
            .lock()
            .map_err(|e| format!("Failed to lock logger: {}", e))?;
        guard.take()
    };
    if let Some(logger) = previous {
        logger.shutdown();
    }

    let logger = AsyncLogger::new()?;
    let mut guard = LOGGER
        .lock()
        .map_err(|e| format!("Failed to lock logger: {}", e))?;
    *guard = Some(logger);
    Ok(())
}

/// Send a message to the global logger.
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

pub fn log_message(level: &str, message: &str) {
    if let Ok(guard) = LOGGER.lock() {
        if let Some(logger) = &*guard {
            logger.log(level, message);
        }
    }
}

// Logging macros
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
        if $crate::logging::is_debug_enabled() {
            $crate::logging::log_message("DEBUG", &format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_logger_waits_after_the_flush_deadline() {
        assert_eq!(log_wait(0, Duration::from_secs(10)), LOG_FLUSH_INTERVAL);
        assert_eq!(log_wait(1, Duration::from_secs(10)), Duration::ZERO);
        assert_eq!(log_wait(10, Duration::ZERO), Duration::ZERO);
        let mut buffer = VecDeque::from(["first".into(), "second".into()]);
        let mut output = Vec::new();
        flush_log_buffer(&mut output, &mut buffer, &mut Instant::now()).unwrap();
        assert_eq!(output, b"first\nsecond\n");
        assert!(buffer.is_empty());
    }
}
