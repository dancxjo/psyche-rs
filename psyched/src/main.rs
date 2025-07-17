use clap::Parser;
use std::path::{Path, PathBuf};
use tokio::fs;
use toml;

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

    /// Directory containing Layka's soul (memory, identity, config)
    #[arg(long, default_value = "soul")]
    pub soul: PathBuf,

    /// Path to TOML pipeline config. If relative, resolved against soul/config/.
    #[arg(long, default_value = "pipeline.toml")]
    pub pipeline: PathBuf,

    /// Beat interval (in milliseconds)
    #[arg(long, default_value_t = 50)]
    pub beat_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // Canonicalize soul path
    let soul = if cli.soul.exists() {
        cli.soul.clone()
    } else {
        PathBuf::from("/etc/soul")
    };

    // Resolve pipeline path
    let pipeline = if cli.pipeline.is_relative() {
        soul.join("config").join(&cli.pipeline)
    } else {
        cli.pipeline.clone()
    };

    // Load identity if present
    let identity_path = soul.join("identity.toml");
    if let Ok(text) = fs::read_to_string(&identity_path).await {
        if let Ok(id) = toml::from_str::<psyched::Identity>(&text) {
            tracing::info!("\u{1F680}  Booting {}...", id.name);
        }
    }

    // Construct LLM registry
    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::ollama::OllamaChat {
            base_url: "http://localhost:11434".into(),
            model: "gemma3:27b".into(),
        }),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });

    let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
        provider: "ollama".into(),
        model: "gemma3:27b".into(),
        capabilities: vec![psyche::llm::LlmCapability::Chat],
    });

    // Kick off orchestrator
    let local = tokio::task::LocalSet::new();
    local
        .run_until(psyched::run(
            cli.socket,
            soul,
            pipeline,
            std::time::Duration::from_millis(cli.beat_ms),
            registry,
            profile,
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
