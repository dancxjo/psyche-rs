use clap::Parser;
use std::path::PathBuf;
use toml;

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

    /// Directory containing Layka's soul
    #[arg(long, alias = "memory-path", default_value = "soul")]
    pub soul: PathBuf,

    /// Path to TOML file describing the distiller pipeline
    #[arg(
        long,
        alias = "config-file",
        default_value = "soul/config/pipeline.toml"
    )]
    pub pipeline: PathBuf,

    /// Beat interval (in milliseconds)
    #[arg(long, default_value_t = 50)]
    pub beat_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let soul = if cli.soul.exists() {
        cli.soul.clone()
    } else {
        PathBuf::from("/etc/soul")
    };
    let pipeline =
        if cli.pipeline == PathBuf::from("soul/config/pipeline.toml") && !cli.pipeline.exists() {
            soul.join("config/pipeline.toml")
        } else {
            cli.pipeline.clone()
        };
    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::mock_chat::MockChat::default()),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });
    let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
        provider: "ollama".into(),
        model: "llama3".into(),
        capabilities: vec![psyche::llm::LlmCapability::Chat],
    });
    let shutdown = shutdown_signal();
    let identity_path = soul.join("identity.toml");
    if let Ok(text) = tokio::fs::read_to_string(&identity_path).await {
        if let Ok(id) = toml::from_str::<psyched::Identity>(&text) {
            tracing::info!("\u{1F680}  Booting {}...", id.name);
        }
    }

    psyched::run(
        cli.socket,
        soul,
        pipeline,
        std::time::Duration::from_millis(cli.beat_ms),
        registry,
        profile,
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
