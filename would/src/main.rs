use clap::Parser;
use daemon_common::{LogLevel, maybe_daemonize};
use psyche::llm::mock_chat::NamedMockChat;
use psyche::llm::{LlmCapability, LlmProfile};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "would", about = "Motor execution daemon")]
struct Cli {
    /// Path to the Unix socket for urge input
    #[arg(long, default_value = "/run/would.sock")]
    socket: PathBuf,

    /// Path to config TOML with [would.motors]
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    /// Optional output socket for motor responses
    #[arg(long)]
    output: Option<PathBuf>,

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
    let llm = std::sync::Arc::new(NamedMockChat::default());
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Chat],
    };
    would::run(cli.socket, cli.config, cli.output, llm, profile).await
}
