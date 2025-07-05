use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::Semaphore;

use crate::stream_util::ReleasingStream;
use crate::{LLMClient, TokenStream};
use ollama_rs::generation::chat::ChatMessage;

/// Wrapper around an [`LLMClient`] that limits concurrent access and ensures
/// FIFO fairness for queued requests.
#[derive(Clone)]
pub struct FairLLM<C> {
    client: C,
    semaphore: Arc<Semaphore>,
}

impl<C> FairLLM<C> {
    /// Create a new [`FairLLM`] wrapping `client` with the given concurrency
    /// limit.
    pub fn new(client: C, max_concurrent: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }
}

#[async_trait]
impl<C> LLMClient for FairLLM<C>
where
    C: LLMClient + Send + Sync,
{
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        tracing::trace!("waiting for llm permit");
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("permit");
        tracing::trace!("llm permit acquired");
        match self.client.chat_stream(messages).await {
            Ok(stream) => {
                let wrapped = ReleasingStream::new(stream, permit);
                Ok(Box::pin(wrapped))
            }
            Err(e) => {
                drop(permit);
                Err(e)
            }
        }
    }

    async fn embed(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("permit");
        self.client.embed(text).await
    }
}

/// Spawn a task that collects the entire response from a [`FairLLM`].
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use async_trait::async_trait;
/// use futures::stream;
/// use psyche_rs::{FairLLM, spawn_fair_llm_task, LLMClient};
/// use ollama_rs::generation::chat::ChatMessage;
/// #[derive(Clone)]
/// struct Dummy;
/// #[async_trait]
/// impl LLMClient for Dummy {
///     async fn chat_stream(
///         &self,
///         _msgs: &[ChatMessage],
///     ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         let stream = stream::once(async { psyche_rs::Token { text: "hi".into() } });
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
/// let llm = Arc::new(FairLLM::new(Dummy, 1));
/// let handle = spawn_fair_llm_task(llm, vec![ChatMessage::user("hi".into())]).await;
/// let text = handle.await.unwrap().unwrap();
/// assert_eq!(text.trim(), "hi");
/// # });
/// ```
pub async fn spawn_fair_llm_task<C>(
    llm: Arc<FairLLM<C>>,
    msgs: Vec<ChatMessage>,
) -> tokio::task::JoinHandle<Result<String, Box<dyn std::error::Error + Send + Sync>>>
where
    C: LLMClient + Send + Sync + 'static,
{
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
