use clap::Parser;
use daemon_common::{maybe_daemonize, LogLevel};
use std::path::PathBuf;
use tokio::fs;
use toml;
use tracing::{debug, trace};

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

    /// Path to LLM config. If relative, resolved against soul/config/.
    #[arg(long, default_value = "llm.toml")]
    pub llm: PathBuf,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    pub log_level: LogLevel,

    /// Qdrant service URL
    #[arg(long, default_value = "http://localhost:6334")]
    pub qdrant_url: String,

    /// Neo4j service URL
    #[arg(long, default_value = "bolt://localhost:7687")]
    pub neo4j_url: String,

    /// Neo4j username
    #[arg(long, default_value = "neo4j")]
    pub neo4j_user: String,

    /// Neo4j password
    #[arg(long, default_value = "password")]
    pub neo4j_pass: String,

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

    // Resolve llm config path
    let llm_cfg = if cli.llm.is_relative() {
        soul.join("config").join(&cli.llm)
    } else {
        cli.llm.clone()
    };

    // Load identity if present
    let identity_path = soul.join("identity.toml");
    if let Ok(text) = fs::read_to_string(&identity_path).await {
        if let Ok(id) = toml::from_str::<psyched::Identity>(&text) {
            tracing::info!("\u{1F680}  Booting {}...", id.name);
        }
    }

    // Construct LLM registry from configuration
    let llms = psyched::llm_config::load_llms(&llm_cfg).await?;
    let first = llms
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no llm"))?;
    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::limited::LimitedChat::new(
            first.chat.clone(),
            first.semaphore.clone(),
        )),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });
    let profile = first.profile.clone();
    let llms: Vec<_> = llms.into_iter().map(std::sync::Arc::new).collect();

    debug!("\u{1F4C1}  Loading identity from {}", identity.display());

    std::env::set_var("QDRANT_URL", &cli.qdrant_url);
    std::env::set_var("NEO4J_URL", &cli.neo4j_url);
    std::env::set_var("NEO4J_USER", &cli.neo4j_user);
    std::env::set_var("NEO4J_PASS", &cli.neo4j_pass);

    // Kick off orchestrator
    let local = tokio::task::LocalSet::new();
    local
        .run_until(psyched::run(
            cli.socket,
            soul,
            identity,
            registry,
            profile,
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
