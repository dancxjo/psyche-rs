use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;

use crate::llm_client::{LLMClient, TokenStream};
use ollama_rs::generation::chat::ChatMessage;

/// Round-robin pool of [`LLMClient`] implementations.
///
/// Each request is delegated to the next client in the pool.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use psyche_rs::{LLMPool, LLMClient};
/// # use async_trait::async_trait;
/// # use futures::{stream, Stream};
/// # use std::{pin::Pin, error::Error};
/// # use ollama_rs::generation::chat::ChatMessage;
/// # struct Dummy;
/// # #[async_trait]
/// # impl LLMClient for Dummy {
/// #   async fn chat_stream(&self, _: &[ChatMessage]) -> Result<psyche_rs::TokenStream, Box<dyn Error + Send + Sync>> {
/// #       Ok(Box::pin(stream::empty()))
/// #   }
/// # }
/// let c1 = Arc::new(Dummy);
/// let pool = LLMPool::new(vec![c1]);
/// let _ = pool.chat_stream(&[]);
/// ```
#[derive(Clone)]
pub struct LLMPool {
    clients: Vec<Arc<dyn LLMClient>>,
    next: Arc<AtomicUsize>,
}

impl LLMPool {
    /// Creates a new pool. Panics if `clients` is empty.
    pub fn new(clients: Vec<Arc<dyn LLMClient>>) -> Self {
        assert!(!clients.is_empty(), "LLM pool cannot be empty");
        Self {
            clients,
            next: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Adds a client to the pool.
    pub fn add(&mut self, client: Arc<dyn LLMClient>) {
        self.clients.push(client);
    }

    fn pick(&self) -> Arc<dyn LLMClient> {
        let idx = self.next.fetch_add(1, Ordering::SeqCst);
        self.clients[idx % self.clients.len()].clone()
    }
}

#[async_trait]
impl LLMClient for LLMPool {
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let client = self.pick();
        client.chat_stream(messages).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::{StreamExt, stream};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordLLM {
        id: usize,
        log: Arc<Mutex<Vec<usize>>>,
    }

    #[async_trait]
    impl LLMClient for RecordLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            self.log.lock().unwrap().push(self.id);
            Ok(Box::pin(stream::empty()))
        }
    }

    #[tokio::test]
    async fn round_robin_distribution() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let c1 = Arc::new(RecordLLM {
            id: 1,
            log: log.clone(),
        });
        let c2 = Arc::new(RecordLLM {
            id: 2,
            log: log.clone(),
        });
        let pool = LLMPool::new(vec![c1, c2]);
        pool.chat_stream(&[]).await.unwrap().next().await;
        pool.chat_stream(&[]).await.unwrap().next().await;
        pool.chat_stream(&[]).await.unwrap().next().await;
        let l = log.lock().unwrap();
        assert_eq!(l.as_slice(), &[1, 2, 1]);
    }
}
