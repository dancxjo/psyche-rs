use clap::Parser;
use std::path::PathBuf;

/// `psyched` — the orchestrator for psycheOS
#[derive(Parser)]
#[command(
    name = "psyched",
    version,
    about = "Core orchestrator for a psycheOS instance"
)]
pub struct Cli {
    /// Path to the Unix domain socket for sensation input
    #[arg(long, default_value = "/run/quick.sock")]
    pub socket: PathBuf,

    /// Path to the raw sensation log
    #[arg(long, default_value = "memory/sensation.jsonl")]
    pub memory: PathBuf,

    /// Path to TOML file describing distillation pipeline
    #[arg(long, default_value = "psyche.toml")]
    pub config: PathBuf,

    /// Beat interval (in milliseconds)
    #[arg(long, default_value_t = 50)]
    pub beat_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let shutdown = shutdown_signal();
    psyched::run(
        cli.socket,
        cli.memory,
        cli.config,
        std::time::Duration::from_millis(cli.beat_ms),
        shutdown,
    )
    .await
}

fn shutdown_signal() -> impl std::future::Future<Output = ()> {
    use tokio::signal::unix::{signal, SignalKind};
    async {
        let mut sigint = signal(SignalKind::interrupt()).unwrap();
        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        tokio::select! {
            _ = sigint.recv() => {},
            _ = sigterm.recv() => {},
        }
    }
}
