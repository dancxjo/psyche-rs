use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing::Level;

#[cfg(feature = "moment-feedback")]
use chrono::Utc;
use futures::{StreamExt, stream};
use ollama_rs::Ollama;
use once_cell::sync::Lazy;
use psyche_rs::{
    Action, Combobulator, Impression, ImpressionSensor, LLMClient, LLMPool, Motor, OllamaLLM,
    Sensor, Wit,
};
#[cfg(feature = "moment-feedback")]
use psyche_rs::{Sensation, SensationSensor};
use serde_json::{Map, Value};

use daringsby::{
    DevelopmentStatus, HeardSelfSensor, Heartbeat, LoggingMotor, Mouth, SelfDiscovery,
    SourceDiscovery, SpeechStream,
};
use std::net::SocketAddr;

const QUICK_PROMPT: &str = include_str!("quick_prompt.txt");
const COMBO_PROMPT: &str = include_str!("combobulator_prompt.txt");
const _WILL_PROMPT: &str = include_str!("will_prompt.txt");

static INSTANT: Lazy<Arc<Mutex<Vec<Impression<String>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));
#[cfg(feature = "moment-feedback")]
static MOMENT: Lazy<Arc<Mutex<Vec<Impression<Impression<String>>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Parser)]
struct Args {
    #[arg(long = "base-url", default_value = "http://localhost:11434", num_args = 1..)]
    base_url: Vec<String>,
    #[arg(long, default_value = "gemma3:27b")]
    model: String,
    /// Host interface for the speech server
    #[arg(long, default_value = "0.0.0.0")]
    host: String,
    /// Port for the speech server
    #[arg(long, default_value_t = 3000)]
    port: u16,
    /// Base URL of the Coqui TTS service
    #[arg(long, default_value = "http://localhost:5002")]
    tts_url: String,
    /// Optional language identifier for TTS
    #[arg(long)]
    language_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let args = Args::parse();
    use tokio::sync::mpsc::unbounded_channel;

    let clients: Vec<Arc<dyn LLMClient>> = args
        .base_url
        .iter()
        .map(|url| {
            let cli = Ollama::try_new(url).expect("ollama client");
            Arc::new(OllamaLLM::new(cli, args.model.clone())) as Arc<dyn LLMClient>
        })
        .collect();
    let llm = Arc::new(LLMPool::new(clients));

    let mouth = Arc::new(Mouth::new(args.tts_url.clone(), args.language_id));
    let audio_rx = mouth.subscribe();
    let text_rx = mouth.subscribe_text();
    let stream = Arc::new(SpeechStream::new(audio_rx, text_rx));
    let app = stream.clone().router();
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    tokio::spawn(async move {
        tracing::info!(%addr, "serving speech stream");
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    let mut quick = Wit::new(llm.clone()).prompt(QUICK_PROMPT).delay_ms(1000);
    let mut combob = Combobulator::new(llm).prompt(COMBO_PROMPT).delay_ms(1000);

    let (tx, rx) = unbounded_channel::<Vec<Impression<String>>>();
    #[cfg(feature = "moment-feedback")]
    let (sens_tx, sens_rx) = unbounded_channel::<Vec<Sensation<String>>>();

    #[cfg_attr(not(feature = "moment-feedback"), allow(unused_mut))]
    let mut sensors: Vec<Box<dyn Sensor<String> + Send>> = vec![
        Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
        Box::new(SelfDiscovery) as Box<dyn Sensor<String> + Send>,
        Box::new(DevelopmentStatus) as Box<dyn Sensor<String> + Send>,
        Box::new(SourceDiscovery) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardSelfSensor::new(stream.subscribe_heard())) as Box<dyn Sensor<String> + Send>,
    ];
    #[cfg(feature = "moment-feedback")]
    sensors.push(Box::new(SensationSensor::new(sens_rx)));

    let mut quick_stream = quick.observe(sensors).await;
    let sensor = ImpressionSensor::new(rx);
    let mut combo_stream = combob.observe(vec![sensor]).await;
    let logger = Arc::new(LoggingMotor);

    let q_instant = INSTANT.clone();
    tokio::spawn(async move {
        while let Some(imps) = quick_stream.next().await {
            *q_instant.lock().unwrap() = imps.clone();
            let _ = tx.send(imps);
        }
    });

    #[cfg(feature = "moment-feedback")]
    {
        let logger_task = logger.clone();
        let mouth_task = mouth.clone();
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
                    let log_text = text.clone();
                    let body = stream::once(async move { log_text }).boxed();
                    let mut action = Action::new("log", Value::Null, body);
                    action.intention.assigned_motor = "log".into();
                    logger_task.perform(action).await.unwrap();

                    let mut map = Map::new();
                    map.insert("speaker_id".into(), Value::String("p234".into()));
                    let speak_text = text;
                    let speak_body = stream::once(async move { speak_text }).boxed();
                    let mut speak = Action::new("speak", Value::Object(map), speak_body);
                    speak.intention.assigned_motor = "speak".into();
                    mouth_task.perform(speak).await.unwrap();
                }
            }
        });
    }

    #[cfg(not(feature = "moment-feedback"))]
    {
        let logger_task = logger.clone();
        let mouth_task = mouth.clone();
        tokio::spawn(async move {
            while let Some(imps) = combo_stream.next().await {
                for imp in imps {
                    let text = imp.how.clone();
                    let log_text = text.clone();
                    let body = stream::once(async move { log_text }).boxed();
                    let mut action = Action::new("log", Value::Null, body);
                    action.intention.assigned_motor = "log".into();
                    logger_task.perform(action).await.unwrap();

                    let mut map = Map::new();
                    map.insert("speaker_id".into(), Value::String("p234".into()));
                    let speak_text = text;
                    let speak_body = stream::once(async move { speak_text }).boxed();
                    let mut speak = Action::new("speak", Value::Object(map), speak_body);
                    speak.intention.assigned_motor = "speak".into();
                    mouth_task.perform(speak).await.unwrap();
                }
            }
        });
    }

    tokio::signal::ctrl_c().await?;
    Ok(())
}
