use clap::Parser;
use daemon_common::{maybe_daemonize, LogLevel};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "seen",
    about = "Image captioning daemon",
    long_about = "Image captioning daemon.\n\nInput Protocol:\n  The stream may optionally begin with a line of the form:\n    @{2025-07-31T14:00:00-07:00}\n  This sets the timestamp used for the input data. If omitted or invalid, the system defaults to the current local time."
)]
struct Cli {
    /// Path to the Unix socket
    #[arg(long, default_value = "/run/psyched/eye.sock")]
    socket: PathBuf,

    /// Base URL for Ollama
    #[arg(long, default_value = "http://localhost:11434")]
    llm_url: String,

    /// Model name
    #[arg(long, default_value = "gemma3:27b")]
    model: String,

    /// Logging verbosity level
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
    seen::run(cli.socket, cli.llm_url, cli.model).await
}
