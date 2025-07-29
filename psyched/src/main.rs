use clap::Parser;
use daemon_common::{maybe_daemonize, LogLevel};
use std::path::PathBuf;
use tokio::fs;
use toml;
use tracing::debug;

/// `psyched` — the orchestrator for psycheOS
#[derive(Parser, Debug)]
#[command(
    name = "psyched",
    version,
    about = "Core orchestrator for a psycheOS instance"
)]
pub struct Cli {
    /// Path to the Unix domain socket for sensation input
    #[arg(long, default_value = "/run/quick.sock")]
    pub socket: PathBuf,

    /// Path to memory recall socket
    #[arg(long, default_value = "/run/memory.sock")]
    pub memory_sock: PathBuf,

    /// Directory containing Layka's soul (memory, identity, config)
    #[arg(long, default_value = "soul")]
    pub soul: PathBuf,

    /// Path to identity file containing Wit configuration.
    #[arg(long, default_value = "identity.toml")]
    pub identity: PathBuf,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    pub log_level: LogLevel,

    /// Run as a background daemon
    #[arg(short = 'd', long)]
    pub daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();

    maybe_daemonize(cli.daemon)?;

    // Canonicalize soul path
    let soul = if cli.soul.exists() {
        cli.soul.clone()
    } else {
        PathBuf::from("/etc/soul")
    };

    // Resolve identity path
    let identity = if cli.identity.is_relative() {
        soul.join(&cli.identity)
    } else {
        cli.identity.clone()
    };

    // Load identity if present
    let identity_path = soul.join("identity.toml");
    if let Ok(text) = fs::read_to_string(&identity_path).await {
        if let Ok(id) = toml::from_str::<psyched::Identity>(&text) {
            tracing::info!("\u{1F680}  Booting {}...", id.name);
        }
    }

    debug!("\u{1F4C1}  Loading identity from {}", identity.display());

    // Kick off orchestrator
    let local = tokio::task::LocalSet::new();
    local
        .run_until(psyched::run(
            cli.socket,
            soul,
            identity,
            cli.memory_sock,
            shutdown_signal(),
        ))
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
