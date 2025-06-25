use psyche_rs::memory::Sensation;
use psyche_rs::{DummyCountenance, DummyLLM, DummyMouth, DummyStore, Psyche};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;

#[tokio::main(flavor = "current_thread")]
async fn main() {
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
        "gemma".into(),
        "You are Pete.".into(),
        2048,
    );

    let local = LocalSet::new();
    local.spawn_local(async move { psyche.tick().await });

    local
        .run_until(async move {
            for i in 0..3 {
                let s = Sensation::new_text(format!("This is event {}", i), "test");
                tx.send(s).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        })
        .await;
}
