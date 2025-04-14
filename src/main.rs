use clap::Parser;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = "Monitors file system events & triggers actions.")]
struct Args {
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!("File config: {}!", args.config.to_string_lossy());
    Ok(())
}
