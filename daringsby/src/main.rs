use async_stream::stream;
use async_trait::async_trait;
use clap::Parser;
use rand::Rng;
use std::sync::Arc;
use tracing::{Level, info};

use llm::builder::{LLMBackend, LLMBuilder};
use psyche_rs::{Motor, MotorCommand, Psyche, Sensation, Sensor, Wit};

struct Heartbeat;

impl Sensor<String> for Heartbeat {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let stream = stream! {
            loop {
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..15)
                };
                tokio::time::sleep(std::time::Duration::from_secs(60 + jitter)).await;
                let now = chrono::Local::now();
                let msg = format!("It's {} o'clock, and I felt my heart beat, so I know I'm alive.", now.format("%H"));
                let s = Sensation { kind: "heartbeat".into(), when: chrono::Utc::now(), what: msg, source: None };
                yield vec![s];
            }
        };
        Box::pin(stream)
    }
}

struct LoggingMotor;

#[async_trait(?Send)]
impl Motor<String> for LoggingMotor {
    async fn execute(&mut self, command: MotorCommand<String>) {
        info!(?command.content, "motor log");
    }
}

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
        .with_max_level(Level::DEBUG)
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
