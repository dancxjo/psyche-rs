use clap::Parser;

/// Command line arguments for the daringsby binary.
#[derive(Parser, Clone)]
pub struct Args {
    /// Base URLs of Ollama servers used by all LLM tasks.
    ///
    /// Specify this flag multiple times to add more servers. If omitted,
    /// `http://localhost:11434` is used. Multiple `--base-url` values are
    /// strongly recommended for parallel LLMs. Concurrency >1 without
    /// multiple servers may overload a single backend.
    #[arg(long = "base-url", num_args(1..), default_value = "http://localhost:11434")]
    pub base_url: Vec<String>,
    /// Max number of concurrent LLM calls. Defaults to the number of
    /// `--base-url` values.
    #[arg(long = "llm-concurrency")]
    pub llm_concurrency: Option<usize>,
    #[arg(long = "quick-model", default_value = "gemma3:27b")]
    pub quick_model: String,
    #[arg(long = "combob-model", default_value = "gemma3:27b")]
    pub combob_model: String,
    #[arg(long = "will-model", default_value = "gemma3:27b")]
    pub will_model: String,
    #[arg(long = "memory-model", default_value = "gemma3:27b")]
    pub memory_model: String,
    /// Base URL of the Ollama server dedicated to the voice loop.
    #[arg(long = "voice-url", default_value = "http://localhost:11434")]
    pub voice_url: String,
    /// Model used for voice generation.
    #[arg(long = "voice-model", default_value = "gemma3:27b")]
    pub voice_model: String,
    /// Model used when generating embeddings.
    #[arg(long = "embedding-model", default_value = "nomic-embed-text")]
    pub embedding_model: String,
    #[arg(long, default_value = "http://localhost:7474")]
    pub neo4j_url: String,
    #[arg(long, default_value = "neo4j")]
    pub neo4j_user: String,
    #[arg(long, default_value = "password")]
    pub neo4j_pass: String,
    #[arg(long, default_value = "http://localhost:6333")]
    pub qdrant_url: String,
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
    #[arg(long, default_value = "http://localhost:5002")]
    pub tts_url: String,
    #[arg(long)]
    pub language_id: Option<String>,
    #[arg(long, default_value = "p234")]
    pub speaker_id: String,
}
