use clap::Parser;
use daringsby::args::Args;
use std::sync::Arc;

use daringsby::{CanvasStream, VisionSensor};
use daringsby::{
    llm_helpers::build_ollama_clients,
    logger,
    motor_helpers::{LLMClients, build_motors},
    mouth_helpers::build_mouth,
    sensor_helpers::build_sensors,
    server_helpers::run_server,
};
use futures::StreamExt;
use psyche_rs::{
    Combobulator, Impression, ImpressionStreamSensor, InMemoryStore, Motor, Sensor, Will, Wit,
    shutdown_signal,
};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger::init();
    let args = Args::parse();

    let (quick_llm, combob_llm, will_llm, memory_llm) = build_ollama_clients(&args);
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

    let store = Arc::new(InMemoryStore::new());
    let (_motors, motor_map) = build_motors(
        &llms,
        mouth.clone(),
        vision.clone(),
        canvas.clone(),
        store.clone(),
    );

    let sensors = build_sensors(stream.clone());

    let (instant_tx, instant_rx) = unbounded_channel();
    let quick = Wit::new(llms.quick.clone()).prompt(include_str!("quick_prompt.txt"));
    let quick_task = tokio::spawn(run_quick(quick, sensors, instant_tx));

    let quick_sensor = ImpressionStreamSensor::new(instant_rx);
    let (situ_tx, situ_rx) = unbounded_channel();
    let combob =
        Combobulator::new(llms.combob.clone()).prompt(include_str!("combobulator_prompt.txt"));
    let combob_task = tokio::spawn(run_combobulator(
        combob,
        vec![Box::new(quick_sensor)],
        situ_tx,
    ));

    let combo_sensor = ImpressionStreamSensor::new(situ_rx);
    let will = Will::new(llms.will.clone()).prompt(include_str!("will_prompt.txt"));
    let will_task = tokio::spawn(run_will(
        will,
        vec![Box::new(combo_sensor)],
        motor_map.clone(),
    ));

    let mut quick_task = Some(quick_task);
    let mut combob_task = Some(combob_task);
    let mut will_task = Some(will_task);

    tokio::select! {
        res = async {
            tokio::try_join!(
                quick_task.take().unwrap(),
                combob_task.take().unwrap(),
                will_task.take().unwrap()
            )
        } => {
            match res {
                Ok(_) => tracing::info!("tasks completed"),
                Err(e) => tracing::error!(error=?e, "task failed"),
            }
        },
        _ = shutdown_signal() => {
            tracing::info!("Shutdown signal received");
        }
    }

    if let Some(handle) = quick_task.take() {
        handle.abort();
        tracing::info!("quick task aborted");
    }
    if let Some(handle) = combob_task.take() {
        handle.abort();
        tracing::info!("combobulator task aborted");
    }
    if let Some(handle) = will_task.take() {
        handle.abort();
        tracing::info!("will task aborted");
    }

    server_handle.abort();
    Ok(())
}

async fn run_quick(
    mut quick: Wit<String>,
    sensors: Vec<Box<dyn Sensor<String> + Send>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<String>>>,
) {
    let mut stream = quick.observe(sensors).await;
    while let Some(batch) = stream.next().await {
        if tx.send(batch).is_err() {
            break;
        }
    }
}

async fn run_combobulator(
    mut combob: Combobulator<String>,
    sensors: Vec<Box<dyn Sensor<Impression<String>> + Send>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<Impression<String>>>>,
) {
    let mut stream = combob.observe(sensors).await;
    while let Some(batch) = stream.next().await {
        if tx.send(batch).is_err() {
            break;
        }
    }
}

async fn run_will(
    mut will: Will<Impression<Impression<String>>>,
    sensors: Vec<Box<dyn Sensor<Impression<Impression<String>>> + Send>>,
    motors: std::collections::HashMap<String, Arc<dyn Motor>>,
) {
    for m in motors.values() {
        will.register_motor(m.as_ref());
    }
    let mut stream = will.observe(sensors).await;
    while let Some(ints) = stream.next().await {
        for intent in ints {
            if let Some(motor) = motors.get(&intent.assigned_motor) {
                let motor = motor.clone();
                tokio::spawn(async move {
                    if let Err(e) = motor.perform(intent).await {
                        tracing::warn!(error=?e, "motor failed");
                    }
                });
            } else {
                tracing::warn!(motor=?intent.assigned_motor, "unknown motor");
            }
        }
    }
}
