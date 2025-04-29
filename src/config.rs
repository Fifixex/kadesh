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

impl Filters {
    pub fn matches(&self, event: &notify::Event) -> bool {
        if let Some(ref kinds) = self.event_kinds {
            if !kinds.iter().any(|k| event_kind_matches(event.kind, k)) {
                return false;
            }
        }

        for path in &event.paths {
            if self
                .ignore_patterns
                .iter()
                .any(|pattern| path_matches_pattern(path, pattern))
            {
                tracing::trace!(?path, ?self.ignore_patterns, "Path matched ignore pattern, skipping.");
                return false;
            }
            if let Some(ref exts) = self.extensions {
                if let Some(ext) = path.extension().and_then(|os| os.to_str()) {
                    let dot_ext = format!(".{}", ext);
                    if !exts.contains(&dot_ext) {
                        tracing::trace!(?path, ?exts, "Path extension mismatch, skipping.");
                        return false;
                    }
                } else {
                    tracing::trace!(
                        ?path,
                        ?exts,
                        "Path has no extension, skipping due to extension filter."
                    );
                    return false;
                }
            }
        }

        true
    }
}

fn path_matches_pattern(path: &Path, pattern: &str) -> bool {
    path.to_str().map_or(false, |s| s.contains(pattern))
}

fn event_kind_matches(kind: EventKind, kind_str: &str) -> bool {
    match kind_str.to_lowercase().as_str() {
        "access" => kind.is_access(),
        "create" => kind.is_create(),
        "modify" | "write" => kind.is_modify() || kind.is_access(),
        "remove" => kind.is_remove(),
        _ => match kind {
            EventKind::Modify(ModifyKind::Data(DataChange::Content))
                if kind_str == "content_change" =>
            {
                true
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) if kind_str == "rename_to" => true,
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) if kind_str == "rename_from" => {
                true
            }
            EventKind::Create(CreateKind::File) if kind_str == "create_file" => true,
            EventKind::Create(CreateKind::Folder) if kind_str == "create_folder" => true,
            EventKind::Remove(RemoveKind::File) if kind_str == "remove_file" => true,
            EventKind::Remove(RemoveKind::Folder) if kind_str == "remove_folder" => true,
            _ => false,
        },
    }
}
