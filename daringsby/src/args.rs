use clap::Parser;

/// Command line arguments for the daringsby binary.
#[derive(Parser, Clone)]
pub struct Args {
    #[arg(long = "quick-url", default_value = "http://localhost:11434")]
    pub quick_url: String,
    #[arg(long = "combob-url", default_value = "http://localhost:11434")]
    pub combob_url: String,
    #[arg(long = "will-url", default_value = "http://localhost:11434")]
    pub will_url: String,
    #[arg(long = "quick-model", default_value = "gemma3:27b")]
    pub quick_model: String,
    #[arg(long = "combob-model", default_value = "gemma3:27b")]
    pub combob_model: String,
    #[arg(long = "will-model", default_value = "gemma3:27b")]
    pub will_model: String,
    #[arg(long = "memory-model", default_value = "gemma3:27b")]
    pub memory_model: String,
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
