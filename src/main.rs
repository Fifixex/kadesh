mod config;
mod errors;

use crate::errors::{AppError, Result};

use clap::Parser;
use config::load_config;
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, info};
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
    Ok(())
}
