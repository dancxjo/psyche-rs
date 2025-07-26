use clap::Parser;
use daemon_common::{maybe_daemonize, LogLevel};
use rememberd::{run, FileStore};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rememberd", about = "Memory JSON-RPC daemon")]
struct Cli {
    /// Path to the Unix socket
    #[arg(long, default_value = "/run/psyche/rememberd.sock")]
    socket: PathBuf,

    /// Directory for JSONL memory logs
    #[arg(long, default_value = "memory")]
    memory_dir: PathBuf,

    /// Logging verbosity
    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    /// Run as a background daemon
    #[arg(short = 'd', long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();

    maybe_daemonize(cli.daemon)?;

    let store = FileStore::new(cli.memory_dir);
    run(cli.socket, store).await
}
