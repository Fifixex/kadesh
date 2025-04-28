use crate::errors::{AppError, Result};
use notify::EventKind;
use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode};
use serde::Deserialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::warn;

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(rename = "watch", default)]
    pub watches: Vec<WatchConfig>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct WatchConfig {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub filter: Filters,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Action {
    pub event: String,
    pub command: String,
}

// TODO: implement this soon
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Filters {
    #[serde(default)]
    pub event_kinds: Option<HashSet<String>>,
    #[serde(default)]
    pub extensions: Option<HashSet<String>>,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

impl WatchConfig {
    pub fn expanded_absolute_path(&self) -> Result<PathBuf> {
        let expanded = shellexpand::full(&self.path).map_err(|e| AppError::PathExpansion {
            path: self.path.clone(),
            source: e,
        })?;
        let path = PathBuf::from(expanded.as_ref());
        path.canonicalize().map_err(|e| {
            warn!(path = ?path, error = %e, "Failed to canonicalize path, using as-is. Ensure it exists and permissions are correct.");
            AppError::Io(e)
        }).or_else(|_| Ok(path))
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_debounce_ms() -> u64 {
    500
}

pub async fn load_config(config_path: &Path) -> Result<Config> {
    let content =
        tokio::fs::read_to_string(config_path)
            .await
            .map_err(|e| AppError::ConfigRead {
                path: config_path.to_path_buf(),
                source: e,
            })?;
    let config: Config = toml::from_str(&content).map_err(|e| AppError::ConfigParse {
        path: config_path.to_path_buf(),
        source: e,
    })?;

    if config.watches.is_empty() {
        warn!("Configuration file loaded, but no [[watch]] sections defined");
    }
    Ok(config)
}
