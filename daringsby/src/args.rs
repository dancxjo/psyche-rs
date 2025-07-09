use clap::Parser;

/// Command line arguments for the daringsby binary.
#[derive(Parser, Clone)]
pub struct Args {
    /// Base URL of the Ollama server used for quick tasks.
    #[arg(long = "quick-url", default_value = "http://localhost:11434")]
    pub quick_url: String,
    /// Base URL of the server used by the Combobulator.
    #[arg(long = "combob-url", default_value = "http://localhost:11434")]
    pub combob_url: String,
    /// Base URL of the server used by the Will wit.
    #[arg(long = "will-url", default_value = "http://localhost:11434")]
    pub will_url: String,
    /// Base URL of the server used for memory operations.
    #[arg(long = "memory-url", default_value = "http://localhost:11434")]
    pub memory_url: String,
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
    #[arg(long = "voice-model", default_value = "gemma3n")]
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
    /// Path to TLS certificate. A self-signed cert will be created if missing.
    #[arg(long, default_value = "cert.pem")]
    pub tls_cert: String,
    /// Path to TLS private key. A self-signed cert will be created if missing.
    #[arg(long, default_value = "key.pem")]
    pub tls_key: String,
    #[arg(long, default_value = "http://localhost:5002")]
    pub tts_url: String,
    #[arg(long)]
    pub language_id: Option<String>,
    #[arg(long, default_value = "p234")]
    pub speaker_id: String,
}
