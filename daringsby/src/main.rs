use clap::Parser;
use daringsby::args::Args;
use std::sync::Arc;

use daringsby::memory_helpers::{ensure_impressions_collection_exists, persist_impression};
use daringsby::{CanvasStream, VisionSensor};
use daringsby::{
    llm_helpers::{build_ollama_clients, build_voice_llm},
    logger,
    motor_helpers::{LLMClients, build_motors},
    mouth_helpers::build_mouth,
    sensor_helpers::{build_ear, build_sensors},
    server_helpers::run_server,
};
use futures::StreamExt;
use futures::stream::BoxStream;
use psyche_rs::MemoryStore;
use psyche_rs::{
    Combobulator, Impression, ImpressionStreamSensor, Motor, MotorExecutor, NeoQdrantMemoryStore,
    Sensor, Voice, Will, Wit, shutdown_signal,
};
use reqwest::Client;
use tokio::sync::mpsc::unbounded_channel;
use url::Url;

async fn run_sensor_loop<I>(
    mut stream: BoxStream<'static, Vec<I>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<I>>,
    name: &str,
) {
    tracing::debug!("{} task started", name);
    while let Some(batch) = stream.next().await {
        if tx.send(batch).is_err() {
            break;
        }
    }
    tracing::info!("{} task finished", name);
}

async fn run_impression_loop<T: serde::Serialize + Clone + Send + 'static>(
    mut stream: BoxStream<'static, Vec<Impression<T>>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<T>>>,
    store: Arc<dyn MemoryStore + Send + Sync>,
    kind: &'static str,
    name: &str,
) {
    tracing::debug!("{} task started", name);
    // Avoid blocking here; persistence runs on background tasks so the
    // main thread can continue processing impressions.
    while let Some(batch) = stream.next().await {
        for imp in &batch {
            let store = Arc::clone(&store);
            let imp = imp.clone();
            // Offload persistence so this loop never blocks on I/O.
            tokio::spawn(async move {
                if let Err(e) = persist_impression(store.as_ref(), imp, kind).await {
                    tracing::warn!(error=?e, "persist failed");
                }
            });
        }
        if tx.send(batch).is_err() {
            break;
        }
    }
    tracing::info!("{} task finished", name);
}

async fn run_voice(
    voice: Voice,
    ear: daringsby::Ear,
    get_situation: Arc<dyn Fn() -> String + Send + Sync>,
    get_instant: Arc<dyn Fn() -> String + Send + Sync>,
    executor: Arc<MotorExecutor>,
) {
    let stream = voice.observe(ear, get_situation, get_instant).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(run_sensor_loop(stream, tx, "voice"));
    while let Some(ints) = rx.recv().await {
        for intent in ints {
            executor.spawn_intention(intent);
        }
    }
    tracing::info!("voice task finished");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger::init();
    let args = Args::parse();

    let (quick_llm, combob_llm, will_llm, memory_llm) = build_ollama_clients(&args);
    let voice_llm = build_voice_llm(&args);
    let llms = LLMClients {
        quick: quick_llm.clone(),
        combob: combob_llm.clone(),
        will: will_llm.clone(),
        memory: memory_llm.clone(),
    };

    let (mouth, stream) = build_mouth(&args).await?;
    let vision = Arc::new(VisionSensor::default());
    let canvas = Arc::new(CanvasStream::default());
    let server_handle = run_server(stream.clone(), vision.clone(), canvas.clone(), &args).await;

    let store = Arc::new(NeoQdrantMemoryStore::new(
        &args.neo4j_url,
        &args.neo4j_user,
        &args.neo4j_pass,
        &args.qdrant_url,
        llms.memory.clone(),
    ));
    let qdrant_url = Url::parse(&args.qdrant_url)?;
    ensure_impressions_collection_exists(&Client::new(), &qdrant_url).await?;
    let (motors, _motor_map) = build_motors(
        &llms,
        mouth.clone(),
        vision.clone(),
        canvas.clone(),
        store.clone(),
    );
    let motors_send: Vec<Arc<dyn Motor + Send + Sync>> = motors
        .iter()
        .cloned()
        .map(|m| m as Arc<dyn Motor + Send + Sync>)
        .collect();
    let executor = Arc::new(MotorExecutor::new(
        motors_send.clone(),
        4,
        16,
        Some(store.clone()),
        None,
    ));

    let sensors = build_sensors(stream.clone());
    let ear = build_ear(stream.clone());
    let voice = Voice::new(voice_llm.clone(), 10)
        .name("Voice")
        .system_prompt(include_str!("prompts/voice_prompt.txt"))
        .delay_ms(0);

    let (instant_tx, instant_rx) = unbounded_channel();
    let (situ_tx, situ_rx) = unbounded_channel();

    let quick_task = {
        let quick = Wit::new(llms.quick.clone())
            .name("Quick")
            .prompt(include_str!("prompts/quick_prompt.txt"));
        tokio::spawn(run_quick(quick, sensors, instant_tx, store.clone()))
    };
    let combob_task = {
        let quick_sensor = ImpressionStreamSensor::new(instant_rx);
        let combob = Combobulator::new(llms.combob.clone())
            .name("Combobulator")
            .prompt(include_str!("prompts/combobulator_prompt.txt"));
        tokio::spawn(run_combobulator(
            combob,
            vec![Box::new(quick_sensor)],
            situ_tx,
            store.clone(),
        ))
    };

    let combo_sensor = ImpressionStreamSensor::new(situ_rx);

    let will_task = {
        let will = Will::new(llms.will.clone())
            .name("Will")
            .prompt(include_str!("prompts/will_prompt.txt"));
        let window = will.window_arc();
        let latest = will.latest_instant_arc();
        let task = tokio::spawn(run_will(
            will,
            vec![Box::new(combo_sensor)],
            executor.clone(),
            motors_send.clone(),
        ));
        (task, window, latest)
    };

    let (will_task, window, latest) = will_task;

    let get_situation = Arc::new(move || psyche_rs::build_timeline(&window));
    let get_instant = Arc::new(move || latest.lock().unwrap().clone());
    let voice_task = tokio::spawn(run_voice(
        voice,
        ear,
        get_situation,
        get_instant,
        executor.clone(),
    ));

    let mut quick = Some(quick_task);
    let mut combob = Some(combob_task);
    let mut will = Some(will_task);
    let mut voice_handle = Some(voice_task);

    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("Shutdown signal received");
            if let Some(h) = quick.take() { h.abort(); }
            if let Some(h) = combob.take() { h.abort(); }
            if let Some(h) = will.take() { h.abort(); }
            if let Some(h) = voice_handle.take() { h.abort(); }
            tracing::info!("Tasks aborted");
        }
        res = async {
            tokio::try_join!(
                quick.take().unwrap(),
                combob.take().unwrap(),
                will.take().unwrap(),
                voice_handle.take().unwrap(),
            )
        } => {
            match res {
                Ok(_) => tracing::info!("All tasks completed successfully"),
                Err(e) => tracing::error!(error=?e, "A task failed"),
            }
        }
    }

    server_handle.abort();
    let _ = server_handle.await;
    tracing::info!("Server aborted");
    Ok(())
}

async fn run_quick(
    mut quick: Wit<String>,
    sensors: Vec<Box<dyn Sensor<String> + Send>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<String>>>,
    store: Arc<dyn MemoryStore + Send + Sync>,
) {
    let stream = quick.observe(sensors).await;
    run_impression_loop(stream, tx, store, "Instant", "quick").await;
}

async fn run_combobulator(
    mut combob: Combobulator<String>,
    sensors: Vec<Box<dyn Sensor<Impression<String>> + Send>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<Impression<String>>>>,
    store: Arc<dyn MemoryStore + Send + Sync>,
) {
    let stream = combob.observe(sensors).await;
    run_impression_loop(stream, tx, store, "Moment", "combobulator").await;
}

async fn run_will(
    mut will: Will<Impression<Impression<String>>>,
    sensors: Vec<Box<dyn Sensor<Impression<Impression<String>>> + Send>>,
    executor: Arc<MotorExecutor>,
    motors: Vec<Arc<dyn Motor + Send + Sync>>,
) {
    tracing::debug!("will task started");
    for m in &motors {
        will.register_motor(m.as_ref());
    }
    let stream = will.observe(sensors).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(run_sensor_loop(stream, tx, "will"));

    while let Some(ints) = rx.recv().await {
        for intent in ints {
            executor.spawn_intention(intent);
        }
    }
    tracing::info!("will task finished");
}
