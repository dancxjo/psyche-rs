//! Memory recall daemon backed by Neo4j + Qdrant
//!
//! ```bash
//! rememberd --qdrant-url http://localhost:6333 \
//!           --neo4j-url bolt://localhost:7687 \
//!           --neo4j-user neo4j --neo4j-pass password
//! ```
use clap::Parser;
use daemon_common::LogLevel;
use std::path::PathBuf;

use neo4rs::Graph;
#[allow(deprecated)]
use qdrant_client::prelude::QdrantClient;

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

    /// Qdrant service URL
    #[arg(long, env = "QDRANT_URL", default_value = "http://localhost:6333")]
    qdrant_url: String,

    /// Neo4j Bolt service URL
    #[arg(long, env = "NEO4J_URL", default_value = "bolt://localhost:7687")]
    neo4j_url: String,

    /// Neo4j username
    #[arg(long, env = "NEO4J_USER", default_value = "neo4j")]
    neo4j_user: String,

    /// Neo4j password
    #[arg(long, env = "NEO4J_PASS", default_value = "password")]
    neo4j_pass: String,
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

    #[allow(deprecated)]
    let qdrant = QdrantClient::from_url(&cli.qdrant_url).build()?;
    let graph = Graph::new(&cli.neo4j_url, cli.neo4j_user, cli.neo4j_pass)?;
    let backend = psyche::memory::QdrantNeo4j { qdrant, graph };
    rememberd::run(cli.socket, cli.output, backend, embed, profile).await
}
