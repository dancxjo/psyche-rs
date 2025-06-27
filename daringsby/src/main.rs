use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing::Level;

use chrono::Utc;
use futures::{StreamExt, stream};
use ollama_rs::Ollama;
use once_cell::sync::Lazy;
use psyche_rs::{
    Action, Combobulator, Impression, ImpressionSensor, Motor, OllamaLLM, Sensation,
    SensationSensor, Sensor, Wit, Witness,
};
use serde_json::Value;

use daringsby::{Heartbeat, LoggingMotor, SelfDiscovery, SourceDiscovery};

const COMBO_PROMPT: &str = include_str!("combobulator_prompt.txt");

static INSTANT: Lazy<Arc<Mutex<Vec<Impression<String>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));
static MOMENT: Lazy<Arc<Mutex<Vec<Impression<Impression<String>>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

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
    use tokio::sync::mpsc::unbounded_channel;

    let client = Ollama::try_new(&args.base_url)?;
    let llm = Arc::new(OllamaLLM::new(client, args.model));

    let mut quick = Wit::new(llm.clone()).delay_ms(1000);
    let mut combob = Combobulator::new(llm).prompt(COMBO_PROMPT).delay_ms(1000);

    let (tx, rx) = unbounded_channel::<Vec<Impression<String>>>();
    let (sens_tx, sens_rx) = unbounded_channel::<Vec<Sensation<String>>>();

    let mut quick_stream = quick
        .observe(vec![
            Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
            Box::new(SelfDiscovery) as Box<dyn Sensor<String> + Send>,
            Box::new(SourceDiscovery) as Box<dyn Sensor<String> + Send>,
            Box::new(SensationSensor::new(sens_rx)), // Senses the moment coming out of the combobulator (?)
        ])
        .await;
    let sensor = ImpressionSensor::new(rx);
    let mut combo_stream = combob.observe(vec![sensor]).await;
    let motor = LoggingMotor;

    let q_instant = INSTANT.clone();
    tokio::spawn(async move {
        while let Some(imps) = quick_stream.next().await {
            *q_instant.lock().unwrap() = imps.clone();
            let _ = tx.send(imps);
        }
    });

    tokio::spawn(async move {
        while let Some(imps) = combo_stream.next().await {
            *MOMENT.lock().unwrap() = imps.clone();
            let sensed: Vec<Sensation<String>> = imps
                .iter()
                .map(|imp| Sensation {
                    kind: "impression".into(),
                    when: Utc::now(),
                    what: imp.how.clone(),
                    source: None,
                })
                .collect();
            let _ = sens_tx.send(sensed);
            for imp in imps {
                let text = imp.how.clone();
                let body = stream::once(async move { text }).boxed();
                let action = Action::new("log", Value::Null, body);
                motor.perform(action).unwrap();
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    Ok(())
}
