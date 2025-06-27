use clap::Parser;
use std::sync::Arc;
use tracing::Level;

use ollama_rs::Ollama;
use psyche_rs::{OllamaLLM, Psyche, Wit};

use daringsby::{Heartbeat, LoggingMotor};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://localhost:11434")]
    base_url: String,
    #[arg(long, default_value = "gemma3:27b")]
    model: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let args = Args::parse();
    let client = Ollama::try_new(&args.base_url)?;
    let llm = Arc::new(OllamaLLM::new(client, args.model));
    let wit = Wit::new(llm);
    let psyche = Psyche::new().sensor(Heartbeat).motor(LoggingMotor).wit(wit);
    psyche.run().await;
    Ok(())
}
