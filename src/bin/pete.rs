use psyche_rs::memory::Sensation;
use psyche_rs::{DummyCountenance, DummyLLM, DummyMouth, DummyStore, Psyche};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let (tx, rx) = mpsc::channel(32);
    let (stop_tx, _stop_rx) = oneshot::channel();

    let psyche = Psyche::new(
        Arc::new(DummyStore::new()),
        Arc::new(DummyLLM),
        Arc::new(DummyMouth),
        Arc::new(DummyCountenance),
        rx,
        stop_tx,
        "gemma".to_string(),
        "You are Pete.".to_string(),
        2048,
    );
    tokio::task::spawn_local(async move {
        psyche.tick().await;
    });

    for i in 0..3 {
        let s = Sensation::new_text(format!("Simulated event {}", i), "test");
        tx.send(s).await?;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    println!("Pete is running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
