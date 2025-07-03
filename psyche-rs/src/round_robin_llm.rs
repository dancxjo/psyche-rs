use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;

use crate::llm_client::{LLMClient, LLMTokenStream};
use ollama_rs::generation::chat::ChatMessage;

/// Round-robin pool of [`LLMClient`] implementations.
///
/// Each request is delegated to the next client in the pool. If that client
/// returns an error, the pool will try the following clients in order until one
/// succeeds. An error is returned only if all clients fail.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use psyche_rs::{RoundRobinLLM, LLMClient};
/// # use async_trait::async_trait;
/// # use futures::{stream, Stream};
/// # use std::{pin::Pin, error::Error};
/// # use ollama_rs::generation::chat::ChatMessage;
/// # struct Dummy;
/// # #[async_trait]
/// # impl LLMClient for Dummy {
/// #   async fn chat_stream(&self, _: &[ChatMessage]) -> Result<psyche_rs::LLMTokenStream, Box<dyn Error + Send + Sync>> {
/// #       Ok(Box::pin(stream::empty()))
/// #   }
/// #   async fn embed(&self, _text: &str) -> Result<Vec<f32>, Box<dyn Error + Send + Sync>> {
/// #       Ok(vec![0.0])
/// #   }
/// # }
/// let c1 = Arc::new(Dummy);
/// let pool = RoundRobinLLM::new(vec![c1]);
/// let _ = pool.chat_stream(&[]);
/// ```
#[derive(Clone)]
pub struct RoundRobinLLM {
    clients: Vec<Arc<dyn LLMClient>>,
    next: Arc<AtomicUsize>,
}

impl RoundRobinLLM {
    /// Creates a new pool. Panics if `clients` is empty.
    pub fn new(clients: Vec<Arc<dyn LLMClient>>) -> Self {
        assert!(!clients.is_empty(), "LLM pool cannot be empty");
        Self {
            clients,
            next: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn pick(&self) -> Arc<dyn LLMClient> {
        let idx = self.next.fetch_add(1, Ordering::Relaxed);
        self.clients[idx % self.clients.len()].clone()
    }
}

#[async_trait]
impl LLMClient for RoundRobinLLM {
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let len = self.clients.len();
        for _ in 0..len {
            let client = self.pick();
            match client.chat_stream(messages).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    tracing::warn!(error = ?e, "llm client failed, trying next");
                }
            }
        }
        Err(Box::new(std::io::Error::other("all llm clients failed")))
    }

    async fn embed(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let len = self.clients.len();
        for _ in 0..len {
            let client = self.pick();
            match client.embed(text).await {
                Ok(vec) => return Ok(vec),
                Err(e) => {
                    tracing::warn!(error = ?e, "llm client failed, trying next");
                }
            }
        }
        Err(Box::new(std::io::Error::other("all llm clients failed")))
    }
}
