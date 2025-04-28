use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: Failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Configuration error: Failed to parse config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Configuration error: Invalid path specified: {0}")]
    InvalidConfigPath(String),

    #[error("File System Watcher Error: {0}")]
    Notify(#[from] notify::Error),

    #[error("Event Debouncer Error: {0}")]
    Debounce(notify::Error),

    #[error("Action Execution Error: Failed to run command '{command}': {source}")]
    ActionExec {
        command: String,
        source: std::io::Error,
    },

    #[error("Path is not valid UTF-8: {0:?}")]
    PathNonUtf8(PathBuf),

    #[error("Failed to expand path '{path}': {source}")]
    PathExpansion {
        path: String,
        source: shellexpand::LookupError<std::env::VarError>,
    },

    #[error("Failed to send event for processing: {0}")]
    EventSend(String),

    #[error("Failed to get absolute path for: {0:?}")]
    AbsolutePath(PathBuf),

    #[error("Action command is empty for event {event_kind:?} in path {path}")]
    EmptyCommand {
        event_kind: String,
        path: PathBuf,
    },
}

pub type Result<T> = std::result::Result<T, AppError>;
