use clap::Parser;
use daringsby::logger;
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(feature = "moment-feedback")]
use chrono::Local;
#[allow(unused_imports)]
use futures::{StreamExt, stream};
use ollama_rs::Ollama;
use once_cell::sync::Lazy;
use psyche_rs::{
    Combobulator, Impression, ImpressionStreamSensor, Intention, LLMClient, Motor, OllamaLLM,
    Sensation, SensationSensor, Sensor, Will, Wit, shutdown_signal,
};
use reqwest::Client;
use url::Url;

use chrono::Utc;
#[cfg(feature = "development-status-sensor")]
use daringsby::DevelopmentStatus;
#[cfg(feature = "self-discovery-sensor")]
use daringsby::SelfDiscovery;
#[cfg(feature = "source-discovery-sensor")]
use daringsby::SourceDiscovery;
use daringsby::{
    CanvasMotor, CanvasStream, HeardSelfSensor, HeardUserSensor, Heartbeat, LogMemoryMotor,
    LoggingMotor, Mouth, RecallMotor, RecallSensor, SourceReadMotor, SourceSearchMotor,
    SourceTreeMotor, SpeechStream, SvgMotor, VisionMotor, VisionSensor,
};
use psyche_rs::{InMemoryStore, MemoryStore, StoredImpression};
use std::net::SocketAddr;
use uuid::Uuid;

const QUICK_PROMPT: &str = include_str!("quick_prompt.txt");
const COMBO_PROMPT: &str = include_str!("combobulator_prompt.txt");
const WILL_PROMPT: &str = include_str!("will_prompt.txt");

static INSTANT: Lazy<Arc<Mutex<Vec<Impression<String>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));
#[cfg(feature = "moment-feedback")]
static MOMENT: Lazy<Arc<Mutex<Vec<Impression<Impression<String>>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Parser)]
struct Args {
    #[arg(long = "quick-url", default_value = "http://localhost:11434")]
    quick_url: String,
    #[arg(long = "combob-url", default_value = "http://localhost:11434")]
    combob_url: String,
    #[arg(long = "will-url", default_value = "http://localhost:11434")]
    will_url: String,
    #[arg(long = "quick-model", default_value = "gemma3:27b")]
    quick_model: String,
    #[arg(long = "combob-model", default_value = "gemma3:27b")]
    combob_model: String,
    #[arg(long = "will-model", default_value = "gemma3:27b")]
    will_model: String,
    #[arg(long = "memory-model", default_value = "gemma3:27b")]
    memory_model: String,
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
    /// Speaker identifier used for TTS requests
    #[arg(long, default_value = "p234")]
    speaker_id: String,
}

fn build_ollama(client: &Client, base: &str) -> Ollama {
    let url = Url::parse(base).expect("invalid base url");
    let host = format!("{}://{}", url.scheme(), url.host_str().expect("no host"));
    let port = url.port_or_known_default().expect("no port");
    Ollama::new_with_client(host, port, client.clone())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger::init();
    let args = Args::parse();
    use tokio::sync::mpsc::unbounded_channel;

    let quick_http = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("quick http client");
    let combob_http = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("combob http client");
    let will_http = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("will http client");

    let quick_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&quick_http, &args.quick_url),
        args.quick_model.clone(),
    ));
    let combob_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&combob_http, &args.combob_url),
        args.combob_model.clone(),
    ));
    let will_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&will_http, &args.will_url),
        args.will_model.clone(),
    ));
    let memory_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&quick_http, &args.quick_url),
        args.memory_model.clone(),
    ));

    let mouth_http = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("tts http client");

    let mouth = Arc::new(Mouth::new(
        mouth_http,
        args.tts_url.clone(),
        args.language_id,
    ));
    let audio_rx = mouth.subscribe();
    let text_rx = mouth.subscribe_text();
    let segment_rx = mouth.subscribe_segments();
    let stream = Arc::new(SpeechStream::new(audio_rx, text_rx, segment_rx));
    let vision_stream = Arc::new(VisionSensor::default());
    let canvas = Arc::new(CanvasStream::default());
    let app = stream
        .clone()
        .router()
        .merge(vision_stream.clone().router())
        .merge(canvas.clone().router());
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let server_handle = tokio::spawn(async move {
        tracing::info!(%addr, "serving speech stream");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind TcpListener");
        axum::serve(listener, app).await.expect("axum serve failed");
    });

    let mut quick = Wit::new(quick_llm.clone())
        .prompt(QUICK_PROMPT)
        .delay_ms(1000);
    let mut combob = Combobulator::new(combob_llm.clone())
        .prompt(COMBO_PROMPT)
        .delay_ms(1000);

    let (tx, rx) = unbounded_channel::<Vec<Impression<String>>>();
    let (look_tx, look_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (canvas_tx, canvas_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (svg_tx, svg_rx) = unbounded_channel::<String>();
    let (thought_tx, thought_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (recall_tx, recall_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (log_mem_tx, log_mem_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (read_tx, read_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (search_tx, search_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (tree_tx, tree_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let store = Arc::new(InMemoryStore::new());
    #[cfg(feature = "moment-feedback")]
    let (sens_tx, sens_rx) = unbounded_channel::<Vec<Sensation<String>>>();

    #[cfg_attr(not(feature = "moment-feedback"), allow(unused_mut))]
    let mut sensors: Vec<Box<dyn Sensor<String> + Send>> = vec![
        Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardSelfSensor::new(stream.subscribe_heard())) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardUserSensor::new(stream.subscribe_user())) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(look_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(canvas_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(thought_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(RecallSensor::new(recall_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(log_mem_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(read_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(search_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(tree_rx)) as Box<dyn Sensor<String> + Send>,
    ];
    #[cfg(feature = "development-status-sensor")]
    sensors.push(Box::new(DevelopmentStatus) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "self-discovery-sensor")]
    sensors.push(Box::new(SelfDiscovery) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "source-discovery-sensor")]
    sensors.push(Box::new(SourceDiscovery) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "moment-feedback")]
    sensors.push(Box::new(SensationSensor::new(sens_rx)));

    let sensor = ImpressionStreamSensor::new(rx);
    let logger = Arc::new(LoggingMotor);
    let vision_motor = Arc::new(VisionMotor::new(
        vision_stream.clone(),
        quick_llm.clone(),
        look_tx,
    ));
    let canvas_motor = Arc::new(CanvasMotor::new(
        canvas.clone(),
        quick_llm.clone(),
        canvas_tx,
    ));
    let svg_motor = Arc::new(SvgMotor::new(svg_tx));
    let recall_motor = Arc::new(RecallMotor::new(
        store.clone(),
        memory_llm.clone(),
        recall_tx,
        5,
    ));
    let log_memory_motor = Arc::new(LogMemoryMotor::new(log_mem_tx));
    let source_read_motor = Arc::new(SourceReadMotor::new(read_tx));
    let source_search_motor = Arc::new(SourceSearchMotor::new(search_tx));
    let source_tree_motor = Arc::new(SourceTreeMotor::new(tree_tx));
    let canvas_handle = {
        let canvas = canvas.clone();
        tokio::spawn(async move {
            let mut rx = svg_rx;
            while let Some(svg) = rx.recv().await {
                canvas.broadcast_svg(svg);
            }
        })
    };
    let _speaker_id = args.speaker_id.clone();

    let (will_tx, will_rx) = unbounded_channel::<Vec<Impression<String>>>();
    let will_sensor = ImpressionStreamSensor::new(will_rx);
    let mut will: Will<Impression<String>> = Will::new(will_llm.clone())
        .prompt(WILL_PROMPT)
        .delay_ms(1000)
        .thoughts(thought_tx);
    will.register_motor(logger.as_ref());
    will.register_motor(vision_motor.as_ref());
    will.register_motor(mouth.as_ref());
    will.register_motor(canvas_motor.as_ref());
    will.register_motor(svg_motor.as_ref());
    will.register_motor(recall_motor.as_ref());
    will.register_motor(log_memory_motor.as_ref());
    will.register_motor(source_read_motor.as_ref());
    will.register_motor(source_search_motor.as_ref());
    will.register_motor(source_tree_motor.as_ref());
    let q_instant = INSTANT.clone();
    let store_quick = store.clone();
    let quick_handle = tokio::spawn(async move {
        let mut quick_stream = quick.observe(sensors).await;
        while let Some(imps) = quick_stream.next().await {
            let mut guard = q_instant.lock().await;
            *guard = imps.clone();
            drop(guard);
            for imp in &imps {
                let stored = StoredImpression {
                    id: Uuid::new_v4().to_string(),
                    kind: "Instant".into(),
                    when: Utc::now(),
                    how: imp.how.clone(),
                    sensation_ids: Vec::new(),
                    impression_ids: Vec::new(),
                };
                let _ = store_quick.store_impression(&stored);
            }
            let _ = tx.send(imps.clone());
            let _ = will_tx.send(imps);
        }
    });

    let combo_logger = logger.clone();
    let combo_handle = tokio::spawn(async move {
        let combo_stream = combob.observe(vec![sensor]).await;
        #[cfg(feature = "moment-feedback")]
        {
            let moment = MOMENT.clone();
            drive_combo_stream(combo_stream, combo_logger.clone(), sens_tx, moment).await;
        }
        #[cfg(not(feature = "moment-feedback"))]
        {
            drive_combo_stream(combo_stream, combo_logger.clone()).await;
        }
    });

    let will_logger = logger.clone();
    let will_handle = tokio::spawn(async move {
        let will_stream = will.observe(vec![will_sensor]).await;
        drive_will_stream(
            will_stream,
            will_logger,
            vision_motor,
            mouth,
            canvas_motor,
            svg_motor,
            recall_motor,
            log_memory_motor,
            source_read_motor,
            source_search_motor,
            source_tree_motor,
        )
        .await;
    });

    shutdown_signal().await;
    tracing::debug!("abort running tasks");
    server_handle.abort();
    canvas_handle.abort();
    quick_handle.abort();
    combo_handle.abort();
    will_handle.abort();
    std::process::exit(0);
}

async fn drive_combo_stream(
    mut combo_stream: impl futures::Stream<Item = Vec<Impression<Impression<String>>>>
    + Unpin
    + Send
    + 'static,
    _logger: Arc<LoggingMotor>,
    #[cfg(feature = "moment-feedback")] sens_tx: tokio::sync::mpsc::UnboundedSender<
        Vec<Sensation<String>>,
    >,
    #[cfg(feature = "moment-feedback")] moment: Arc<Mutex<Vec<Impression<Impression<String>>>>>,
) {
    use futures::StreamExt;

    while let Some(_imps) = combo_stream.next().await {
        #[cfg(feature = "moment-feedback")]
        {
            let mut guard = moment.lock().await;
            *guard = imps.clone();
            drop(guard);
            let sensed: Vec<Sensation<String>> = imps
                .iter()
                .map(|imp| Sensation {
                    kind: "impression".into(),
                    when: Local::now(),
                    what: imp.how.clone(),
                    source: None,
                })
                .collect();
            let _ = sens_tx.send(sensed);
        }
        // Logging is now handled exclusively by the Will. The combo stream only
        // surfaces impressions without directly invoking the log motor.
    }
}

async fn drive_will_stream<M>(
    mut will_stream: impl futures::Stream<Item = Vec<Intention>> + Unpin + Send + 'static,
    logger: Arc<LoggingMotor>,
    vision_motor: Arc<VisionMotor>,
    mouth: Arc<Mouth>,
    canvas: Arc<CanvasMotor>,
    drawer: Arc<SvgMotor>,
    recall: Arc<RecallMotor<M>>,
    log_memory: Arc<LogMemoryMotor>,
    source_read: Arc<SourceReadMotor>,
    source_search: Arc<SourceSearchMotor>,
    source_tree: Arc<SourceTreeMotor>,
) where
    M: psyche_rs::MemoryStore + Send + Sync + 'static,
{
    use futures::StreamExt;

    while let Some(ints) = will_stream.next().await {
        for intent in ints {
            match intent.assigned_motor.as_str() {
                "log" => {
                    logger.perform(intent).await.expect("logging motor failed");
                }
                "look" => {
                    vision_motor
                        .perform(intent)
                        .await
                        .expect("look motor failed");
                }
                "say" => {
                    mouth.perform(intent).await.expect("mouth motor failed");
                }
                "canvas" => {
                    canvas.perform(intent).await.expect("canvas motor failed");
                }
                "draw" => {
                    drawer.perform(intent).await.expect("svg motor failed");
                }
                "recall" => {
                    recall.perform(intent).await.expect("recall motor failed");
                }
                "read_log_memory" => {
                    log_memory
                        .perform(intent)
                        .await
                        .expect("log memory motor failed");
                }
                "read_source" => {
                    source_read
                        .perform(intent)
                        .await
                        .expect("read source motor failed");
                }
                "search_source" => {
                    source_search
                        .perform(intent)
                        .await
                        .expect("search source motor failed");
                }
                "source_tree" => {
                    source_tree
                        .perform(intent)
                        .await
                        .expect("source tree motor failed");
                }
                _ => {
                    tracing::warn!(motor = %intent.assigned_motor, "unknown motor");
                }
            }
        }
    }
}
