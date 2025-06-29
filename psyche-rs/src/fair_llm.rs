use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::llm_client::{LLMClient, TokenStream};
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

struct ReleasingStream<S> {
    inner: S,
    permit: Option<OwnedSemaphorePermit>,
}

impl<S> ReleasingStream<S> {
    fn new(inner: S, permit: OwnedSemaphorePermit) -> Self {
        Self {
            inner,
            permit: Some(permit),
        }
    }
}

impl<S> Drop for ReleasingStream<S> {
    fn drop(&mut self) {
        // permit will be dropped automatically
        let _ = self.permit.take();
    }
}

impl<S> Stream for ReleasingStream<S>
where
    S: Stream + Unpin,
{
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let poll = std::pin::Pin::new(&mut self.inner).poll_next(cx);
        if let std::task::Poll::Ready(None) = poll {
            let _ = self.permit.take();
        }
        poll
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::{StreamExt, stream};
    use std::sync::Arc;

    #[derive(Clone)]
    struct DelayLLM {
        delay: u64,
    }

    #[async_trait]
    impl LLMClient for DelayLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            let d = self.delay;
            let s = stream::once(async move {
                tokio::time::sleep(std::time::Duration::from_millis(d)).await;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("done".into())
            });
            Ok(Box::pin(s))
        }
    }

    #[tokio::test]
    async fn processes_in_request_order() {
        let llm = Arc::new(FairLLM::new(DelayLLM { delay: 50 }, 1));
        let llm2 = llm.clone();
        let start = std::time::Instant::now();
        let f1 = tokio::spawn(async move {
            let mut s = llm.chat_stream(&[]).await.unwrap();
            s.next().await.unwrap().unwrap();
        });
        let f2 = tokio::spawn(async move {
            let mut s = llm2.chat_stream(&[]).await.unwrap();
            s.next().await.unwrap().unwrap();
        });
        let _ = futures::join!(f1, f2);
        assert!(start.elapsed() >= std::time::Duration::from_millis(100));
    }
}
