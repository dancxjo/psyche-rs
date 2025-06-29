use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::llm_client::{LLMClient, LLMTokenStream};
use crate::stream_util::ReleasingStream;
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
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
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
}
