use clap::Parser;
use std::sync::Arc;
use tracing::Level;

use llm::builder::{LLMBackend, LLMBuilder};
use psyche_rs::{Psyche, Wit};

use daringsby::{Heartbeat, LoggingMotor};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://localhost:14434")]
    base_url: String,
    #[arg(long, default_value = "gemma3:27b")]
    model: String,
    #[arg(long, default_value = "ollama")]
    backend: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let args = Args::parse();
    let backend = args.backend.parse::<LLMBackend>()?;
    let llm = LLMBuilder::new()
        .backend(backend)
        .base_url(args.base_url)
        .model(args.model)
        .build()?;
    let wit = Wit::new(Arc::from(llm));
    let psyche = Psyche::new().sensor(Heartbeat).motor(LoggingMotor).wit(wit);
    psyche.run().await;
    Ok(())
}
