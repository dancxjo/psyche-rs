use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::{pin::Pin, sync::Arc};

use ollama_rs::generation::chat::ChatMessage;

/// Stream of LLM-generated text fragments.
pub type LLMTokenStream =
    Pin<Box<dyn Stream<Item = Result<String, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

/// Common interface for chat-based LLMs.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Streams text fragments in response to chat messages.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>>;
}

/// Spawn a task that collects the entire response from `llm` into a `String`.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use async_trait::async_trait;
/// use futures::stream;
/// use psyche_rs::{spawn_llm_task, LLMClient};
/// use ollama_rs::generation::chat::ChatMessage;
/// #[derive(Clone)]
/// struct Dummy;
/// #[async_trait]
/// impl LLMClient for Dummy {
///     async fn chat_stream(
///         &self,
///         _: &[ChatMessage],
///     ) -> Result<psyche_rs::LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(Box::pin(stream::once(async { Ok("hello ".to_string()) })))
///     }
/// }
/// # tokio_test::block_on(async {
/// let llm = Arc::new(Dummy);
/// let handle = spawn_llm_task(llm, vec![ChatMessage::user("hi".into())]).await;
/// let text = handle.await.unwrap().unwrap();
/// assert_eq!(text.trim(), "hello");
/// # });
/// ```
pub async fn spawn_llm_task(
    llm: Arc<dyn LLMClient>,
    msgs: Vec<ChatMessage>,
) -> tokio::task::JoinHandle<Result<String, Box<dyn std::error::Error + Send + Sync>>> {
    tokio::spawn(async move {
        let mut stream = llm.chat_stream(&msgs).await?;
        let mut out = String::new();
        while let Some(tok) = stream.next().await {
            out.push_str(&tok?);
        }
        tracing::debug!(%out, "llm full response");
        Ok(out)
    })
}
