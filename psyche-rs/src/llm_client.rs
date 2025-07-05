use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;

use crate::llm_types::TokenStream;
use ollama_rs::generation::chat::ChatMessage;

/// Common interface for chat-based LLMs.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Streams text fragments in response to chat messages.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>>;

    /// Generate an embedding vector for the provided text.
    async fn embed(&self, text: &str)
    -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>>;
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
///     ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         let stream = stream::once(async { psyche_rs::Token { text: "hello ".into() } });
///         Ok(Box::pin(stream))
///     }
///     async fn embed(
///         &self,
///         _text: &str,
///     ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(vec![0.0])
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
            out.push_str(&tok.text);
        }
        tracing::debug!(%out, "llm full response");
        Ok(out)
    })
}
