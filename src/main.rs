mod actions;
mod config;
mod errors;

use crate::errors::{AppError, Result};

use clap::Parser;
use config::{WatchConfig, event_kind_to_primary_string, load_config};
use notify::{INotifyWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::{Instrument, debug, error, info, instrument, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

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
    let runtime_handle = tokio::runtime::Handle::current();
    let mut debouncer = new_debouncer(
        Duration::from_millis(config.debounce_ms),
        None,
        move |result| {
            let tx = event_tx.clone();
            let handle = runtime_handle.clone();
            handle.spawn(async move {
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

    let config_clone = Arc::clone(&config);
    let event_processor = tokio::spawn(async move {
        while let Some(result) = event_rx.recv().await {
            match result {
                Ok(events) => {
                    for event in events {
                        let cfg = Arc::clone(&config_clone);
                        tokio::spawn(
                            process_event(event, cfg)
                                .instrument(tracing::info_span!("process_event")),
                        );
                    }
                }
                Err(errors) => {
                    for error in errors {
                        error!(error = %error, "Debouncer error");
                    }
                }
            }
        }
        info!("Event processing loop finished.");
    });

    tokio::select! {
      _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C received. Shutting down...");
        }
      _ = event_processor => {
        warn!("Event processor task completed unexpectedly.");

      }
    };

    drop(debouncer);
    info!("Watcher stopped. Exiting.");

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

#[instrument(skip(event, config), fields(kind = ?event.kind, paths = ?event.paths))]
async fn process_event(event: DebouncedEvent, config: Arc<config::Config>) {
    debug!("Processing event");

    for watch_config in &config.watches {
        let is_relevant = event
            .paths
            .iter()
            .any(|p| match watch_config.expanded_absolute_path() {
                Ok(watch_root) => p.starts_with(&watch_root),
                Err(_) => false,
            });

        if !is_relevant {
            continue;
        }

        if !watch_config.filters.matches(&event) {
            debug!(config_path = %watch_config.path, "Event filtered out");
            continue;
        }

        let primary_kind_str = event_kind_to_primary_string(event.kind);

        for action in &watch_config.actions {
            let action_event_str = action.event.to_lowercase();
            let mut matched = false;

            if action_event_str == "any" {
                matched = true;
            } else if let Some(primary_kind) = primary_kind_str {
                if action_event_str == primary_kind {
                    matched = true;
                }
            }

            if matched {
                if action.command.trim().is_empty() {
                    warn!(event = %action.event, config_path = %watch_config.path, "Action has empty command, skipping.");
                    continue;
                }
                for path in &event.paths {
                    let cmd = action.command.clone();
                    let p = path.clone();
                    tokio::spawn(async move {
                        if let Err(e) = actions::execute_action(&cmd, &p).await {
                            error!(command = %cmd, path = %p.display(), error = %e, "Action execution failed");
                        }
                    }.instrument(tracing::info_span!("execute_action", command = %action.command)));
                }
                break;
            }
        }
    }
}
