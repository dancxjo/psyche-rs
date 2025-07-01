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
use psyche_rs::{InMemoryStore, shutdown_signal};

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
    let (_motors, _map) = build_motors(
        &llms,
        mouth.clone(),
        vision.clone(),
        canvas.clone(),
        store.clone(),
    );

    let _sensors = build_sensors(stream.clone());

    // spawn wits and will as before (omitted for brevity)

    shutdown_signal().await;
    server_handle.abort();
    Ok(())
}
