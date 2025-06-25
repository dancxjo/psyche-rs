use crate::memory::Sensation;
use crate::{DummyCountenance, DummyLLM, DummyMouth, DummyStore, Psyche};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;

/// Launch a basic Pete instance using in-memory components.
///
/// This spawns [`Psyche::tick`] on a background task and feeds it a few
/// example [`Sensation`]s before returning.
///
/// ```no_run
/// # use psyche_rs::pete;
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     pete::launch_default_pete().await
/// }
/// ```
pub async fn launch_default_pete() -> anyhow::Result<()> {
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
    local.spawn_local(async move {
        psyche.tick().await;
    });

    local
        .run_until(async move {
            for i in 0..3 {
                tx.send(Sensation::new_text(format!("This is test {}", i), "cli"))
                    .await?;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await?;

    Ok(())
}
