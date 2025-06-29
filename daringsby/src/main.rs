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
    Action, Combobulator, Impression, ImpressionStreamSensor, Intention, LLMClient, Motor,
    OllamaLLM, RoundRobinLLM, Sensation, SensationSensor, Sensor, Will, Wit,
};

#[cfg(feature = "development-status-sensor")]
use daringsby::DevelopmentStatus;
#[cfg(feature = "self-discovery-sensor")]
use daringsby::SelfDiscovery;
#[cfg(feature = "source-discovery-sensor")]
use daringsby::SourceDiscovery;
use daringsby::{
    CanvasMotor, CanvasStream, HeardSelfSensor, HeardUserSensor, Heartbeat, LoggingMotor, LookMotor, LookStream,
    Mouth, SpeechStream, SvgMotor,
};
use std::net::SocketAddr;

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
    /// Speaker identifier used for TTS requests
    #[arg(long, default_value = "p234")]
    speaker_id: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger::init();
    let args = Args::parse();
    use tokio::sync::mpsc::unbounded_channel;

    let clients: Vec<Arc<dyn LLMClient>> = args
        .base_url
        .iter()
        .map(|url| {
            let cli = Ollama::try_new(url).expect("failed to create Ollama client");
            Arc::new(OllamaLLM::new(cli, args.model.clone())) as Arc<dyn LLMClient>
        })
        .collect();
    let llm = Arc::new(RoundRobinLLM::new(clients));

    let mouth = Arc::new(Mouth::new(args.tts_url.clone(), args.language_id));
    let audio_rx = mouth.subscribe();
    let text_rx = mouth.subscribe_text();
    let segment_rx = mouth.subscribe_segments();
    let stream = Arc::new(SpeechStream::new(audio_rx, text_rx, segment_rx));
    let vision = Arc::new(LookStream::default());
    let canvas = Arc::new(CanvasStream::default());
    let app = stream
        .clone()
        .router()
        .merge(vision.clone().router())
        .merge(canvas.clone().router());
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    tokio::spawn(async move {
        tracing::info!(%addr, "serving speech stream");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind TcpListener");
        axum::serve(listener, app).await.expect("axum serve failed");
    });

    let mut quick = Wit::new(llm.clone()).prompt(QUICK_PROMPT).delay_ms(1000);
    let mut combob = Combobulator::new(llm.clone())
        .prompt(COMBO_PROMPT)
        .delay_ms(1000);

    let (tx, rx) = unbounded_channel::<Vec<Impression<String>>>();
    let (look_tx, look_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (canvas_tx, canvas_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (svg_tx, svg_rx) = unbounded_channel::<String>();
    #[cfg(feature = "moment-feedback")]
    let (sens_tx, sens_rx) = unbounded_channel::<Vec<Sensation<String>>>();

    #[cfg_attr(not(feature = "moment-feedback"), allow(unused_mut))]
    let mut sensors: Vec<Box<dyn Sensor<String> + Send>> = vec![
        Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardSelfSensor::new(stream.subscribe_heard())) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardUserSensor::new(stream.subscribe_user())) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(look_rx)) as Box<dyn Sensor<String> + Send>,
        Box::new(SensationSensor::new(canvas_rx)) as Box<dyn Sensor<String> + Send>,
    ];
    #[cfg(feature = "development-status-sensor")]
    sensors.push(Box::new(DevelopmentStatus) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "self-discovery-sensor")]
    sensors.push(Box::new(SelfDiscovery) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "source-discovery-sensor")]
    sensors.push(Box::new(SourceDiscovery) as Box<dyn Sensor<String> + Send>);
    #[cfg(feature = "moment-feedback")]
    sensors.push(Box::new(SensationSensor::new(sens_rx)));

    let mut quick_stream = quick.observe(sensors).await;
    let sensor = ImpressionStreamSensor::new(rx);
    let combo_stream = combob.observe(vec![sensor]).await;
    let logger = Arc::new(LoggingMotor);
    let looker = Arc::new(LookMotor::new(vision.clone(), llm.clone(), look_tx));
    let canvas_motor = Arc::new(CanvasMotor::new(canvas.clone(), llm.clone(), canvas_tx));
    let svg_motor = Arc::new(SvgMotor::new(svg_tx));
    {
        let canvas = canvas.clone();
        tokio::spawn(async move {
            let mut rx = svg_rx;
            while let Some(svg) = rx.recv().await {
                canvas.broadcast_svg(svg);
            }
        });
    }
    let _speaker_id = args.speaker_id.clone();

    let (will_tx, will_rx) = unbounded_channel::<Vec<Impression<String>>>();
    let will_sensor = ImpressionStreamSensor::new(will_rx);
    let mut will = Will::new(llm.clone()).prompt(WILL_PROMPT).delay_ms(1000);
    will.register_motor(logger.as_ref());
    will.register_motor(looker.as_ref());
    will.register_motor(mouth.as_ref());
    will.register_motor(canvas_motor.as_ref());
    will.register_motor(svg_motor.as_ref());
    let will_stream = will.observe(vec![will_sensor]).await;

    let q_instant = INSTANT.clone();
    tokio::spawn(async move {
        while let Some(imps) = quick_stream.next().await {
            let mut guard = q_instant.lock().await;
            *guard = imps.clone();
            drop(guard);
            let _ = tx.send(imps.clone());
            let _ = will_tx.send(imps);
        }
    });

    #[cfg(feature = "moment-feedback")]
    {
        let moment = MOMENT.clone();
        tokio::spawn(drive_combo_stream(
            combo_stream,
            logger.clone(),
            sens_tx,
            moment,
        ));
    }

    #[cfg(not(feature = "moment-feedback"))]
    {
        tokio::spawn(drive_combo_stream(combo_stream, logger.clone()));
    }

    tokio::spawn(drive_will_stream(
        will_stream,
        logger,
        looker,
        mouth,
        canvas_motor,
        svg_motor,
    ));

    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn drive_combo_stream(
    mut combo_stream: impl futures::Stream<Item = Vec<Impression<Impression<String>>>>
    + Unpin
    + Send
    + 'static,
    logger: Arc<LoggingMotor>,
    #[cfg(feature = "moment-feedback")] sens_tx: tokio::sync::mpsc::UnboundedSender<
        Vec<Sensation<String>>,
    >,
    #[cfg(feature = "moment-feedback")] moment: Arc<Mutex<Vec<Impression<Impression<String>>>>>,
) {
    use futures::{StreamExt, stream};
    use serde_json::Value;

    while let Some(imps) = combo_stream.next().await {
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
        for imp in imps {
            let text = imp.how.clone();
            let body = stream::once(async move { text }).boxed();
            let action = Action::new("log", Value::Null, body);
            let intention = Intention::to(action).assign("log");
            logger
                .perform(intention)
                .await
                .expect("logging motor failed");
        }
    }
}

async fn drive_will_stream(
    mut will_stream: impl futures::Stream<Item = Vec<Intention>> + Unpin + Send + 'static,
    logger: Arc<LoggingMotor>,
    looker: Arc<LookMotor>,
    mouth: Arc<Mouth>,
    canvas: Arc<CanvasMotor>,
    drawer: Arc<SvgMotor>,
) {
    use futures::StreamExt;

    while let Some(ints) = will_stream.next().await {
        for intent in ints {
            match intent.assigned_motor.as_str() {
                "log" => {
                    logger.perform(intent).await.expect("logging motor failed");
                }
                "look" => {
                    looker.perform(intent).await.expect("look motor failed");
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
                _ => {
                    tracing::warn!(motor = %intent.assigned_motor, "unknown motor");
                }
            }
        }
    }
}
