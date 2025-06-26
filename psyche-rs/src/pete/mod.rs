use crate::memory::{MemoryStore, Sensation};
use crate::{
    DummyCountenance, DummyMouth, DummyStore, Psyche, countenance::Countenance, mouth::Mouth,
};
use llm::chat::ChatProvider;

struct NoopLLM;

#[async_trait::async_trait]
impl ChatProvider for NoopLLM {
    async fn chat_with_tools(
        &self,
        _messages: &[llm::chat::ChatMessage],
        _tools: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn llm::chat::ChatResponse>, llm::error::LLMError> {
        Ok(Box::new(NoopResp))
    }

    async fn chat_stream(
        &self,
        _messages: &[llm::chat::ChatMessage],
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
        >,
        llm::error::LLMError,
    > {
        Ok(Box::pin(futures_util::stream::empty()))
    }
}

#[derive(Debug)]
struct NoopResp;

impl llm::chat::ChatResponse for NoopResp {
    fn text(&self) -> Option<String> {
        Some(String::new())
    }
    fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
        None
    }
}

impl std::fmt::Display for NoopResp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "")
    }
}
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;

/// Launch a basic Pete instance using in-memory components.
///
/// This spawns Pete's cognition on a local task set and feeds it a few
/// example [`Sensation`]s before returning.
///
/// ```no_run
/// # use psyche_rs::pete;
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     pete::launch_default_pete().await
/// }
/// ```
///
/// Build a [`Psyche`] using the provided dependencies.
pub fn build_pete(
    store: Arc<dyn MemoryStore>,
    llm: Arc<dyn ChatProvider>,
    _mouth: Arc<dyn Mouth>,
    _countenance: Arc<dyn Countenance>,
) -> (Psyche, mpsc::Sender<Sensation>, oneshot::Receiver<()>) {
    let (tx, rx) = mpsc::channel(32);
    let (stop_tx, stop_rx) = oneshot::channel();

    let psyche = Psyche::new(store, llm);
    // forward sensations from tx into psyche
    let sender = psyche.quick.sender.clone();
    tokio::task::spawn_local(async move {
        let mut rx = rx;
        while let Some(s) = rx.recv().await {
            let _ = sender.send(s).await;
        }
        let _ = stop_tx.send(());
    });

    (psyche, tx, stop_rx)
}

pub async fn launch_default_pete() -> anyhow::Result<()> {
    let (_psyche, tx, stop_rx) = build_pete(
        Arc::new(DummyStore::new()),
        Arc::new(NoopLLM),
        Arc::new(DummyMouth),
        Arc::new(DummyCountenance),
    );

    let local = LocalSet::new();
    local
        .run_until(async move {
            for i in 0..3 {
                tx.send(Sensation::new_text(format!("This is test {}", i), "cli"))
                    .await?;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }

            drop(tx);

            tokio::pin!(stop_rx);
            tokio::select! {
                _ = signal::ctrl_c() => {
                    println!("Ctrl-C received. Shutting down...");
                    let _ = stop_rx.await;
                }
                _ = &mut stop_rx => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .await?;

    Ok(())
}
