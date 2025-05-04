mod config;
mod errors;

use crate::errors::{AppError, Result};

use clap::Parser;
use config::{WatchConfig, load_config};
use notify::{INotifyWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = "Monitors file system events & triggers actions."
)]
struct Args {
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config = match load_config(&args.config).await {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);

            if let AppError::ConfigParse { path, source } = &e {
                eprintln!(" -> Parsing error in: {:?}: {}", path, source);
            } else if let AppError::ConfigRead { path, source } = &e {
                eprintln!(" -> Reading error for {:?}: {}", path, source);
            }
            return Err(e);
        }
    };

    let log_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(log_filter)
        .with_span_events(FmtSpan::CLOSE)
        .init();

    info!("Logging initialized!");
    debug!(config = ?config, "Loaded configuration");

    let (event_tx, mut event_rx) = mpsc::channel::<DebounceEventResult>(100);
    let mut debouncer = new_debouncer(
        Duration::from_millis(config.debounce_ms),
        None,
        move |result| {
            let tx = event_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = tx.send(result).await {
                    error!("Failed to send debounced event: {}", e);
                }
            });
        },
    )
    .map_err(AppError::Debounce)?;

    for watch_config in &config.watches {
        match setup_watch(&mut debouncer, watch_config) {
            Ok(abs_path) => info!(
              path = %abs_path.display(),
              recursive = watch_config.recursive,
              "Started watching"
            ),
            Err(e) => error!(
              config_path = %watch_config.path,
              error = %e,
              "Failed to set up watch, skipping this entry"
            ),
        }
    }

    if config.watches.is_empty() {
        warn!("No valid watch paths configured. Exiting.");
        return Ok(());
    }

    info!("File system monitor started. Press Ctrl+C to stop.");

    Ok(())
}

fn setup_watch(
    watcher: &mut Debouncer<INotifyWatcher, NoCache>,
    watch_config: &WatchConfig,
) -> Result<PathBuf> {
    let path_to_watch = watch_config.expanded_absolute_path()?;

    if !path_to_watch.exists() {
        warn!(path = %path_to_watch.display(), "Watch path does not exist. It will watched if created later.");
    } else if !path_to_watch.is_dir() && watch_config.recursive {
        warn!(path = %path_to_watch.display(), "Recurisive wathc requested on a file, treating as non-recursive.");
    }

    let rec_mode = if watch_config.recursive && path_to_watch.is_dir() {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    watcher.watch(&path_to_watch, rec_mode)?;

    Ok(path_to_watch)
}
