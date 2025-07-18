use clap::Parser;
use daemon_common::LogLevel;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rememberd", about = "Memory recall daemon")]
struct Cli {
    /// Path to listen for recall queries
    #[arg(long, default_value = "/run/memory.sock")]
    socket: PathBuf,

    /// Path to output sensations
    #[arg(long, default_value = "/run/quick.sock")]
    output: PathBuf,

    /// Optional memory directory (unused for now)
    #[arg(long)]
    memory_dir: Option<PathBuf>,

    /// Path to LLM profile for embedding
    #[arg(long)]
    llm_profile: Option<PathBuf>,

    /// Logging verbosity
    #[arg(long, default_value = "info")]
    log_level: LogLevel,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();

    let (embed, profile): (Box<dyn psyche::llm::CanEmbed>, psyche::llm::LlmProfile) =
        if let Some(path) = cli.llm_profile.as_deref() {
            let (reg, prof) = psyched::llm_config::load_first_llm(path).await?;
            (reg.embed, prof)
        } else {
            (
                Box::new(psyche::llm::mock_embed::MockEmbed::default())
                    as Box<dyn psyche::llm::CanEmbed>,
                psyche::llm::LlmProfile {
                    provider: "mock".into(),
                    model: "mock".into(),
                    capabilities: vec![psyche::llm::LlmCapability::Embedding],
                },
            )
        };

    let backend = psyche::memory::InMemoryBackend::default();
    rememberd::run(cli.socket, cli.output, backend, embed, profile).await
}
