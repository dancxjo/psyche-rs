use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use url::Url;

use crate::{
    OllamaLLM, RoundRobinLLM,
    llm_client::{LLMClient, LLMTokenStream},
};
use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;

/// Pool of LLM backends identified by base URLs.
///
/// Each request is delegated to the next backend in round-robin order.
/// This allows distributing load across multiple Ollama instances.
///
/// # Examples
/// ```ignore
/// let urls = vec!["http://localhost:11434".to_string()];
/// let pool = LLMPool::new(urls, "llama");
/// ```
pub struct LLMPool {
    inner: RoundRobinLLM,
}

impl LLMPool {
    /// Create a new pool from base URLs and model name.
    pub fn new(urls: Vec<String>, model: impl Into<String>) -> Self {
        assert!(!urls.is_empty(), "LLM URLs cannot be empty");
        let model = model.into();
        let http = Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .expect("http client");
        let clients: Vec<Arc<dyn LLMClient>> = urls
            .into_iter()
            .map(|u| new_client(&http, &u, &model))
            .collect();
        Self {
            inner: RoundRobinLLM::new(clients),
        }
    }

    /// Create a pool from an existing [`RoundRobinLLM`]. Used in tests.
    #[cfg(test)]
    pub(crate) fn from_round_robin(inner: RoundRobinLLM) -> Self {
        Self { inner }
    }

    /// Spawn a task that collects all tokens into a `String`.
    pub async fn spawn_llm_task(
        &self,
        msgs: Vec<ChatMessage>,
    ) -> tokio::task::JoinHandle<Result<String, Box<dyn std::error::Error + Send + Sync>>> {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let mut stream = inner.chat_stream(&msgs).await?;
            let mut out = String::new();
            while let Some(tok) = stream.next().await {
                out.push_str(&tok?);
            }
            Ok(out)
        })
    }

    /// Stream tokens from the next backend.
    pub async fn stream_llm_response(
        &self,
        msgs: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.inner.chat_stream(msgs).await
    }
}

fn new_client(http: &Client, base: &str, model: &str) -> Arc<dyn LLMClient> {
    let url = Url::parse(base).expect("invalid base url");
    let host = format!("{}://{}", url.scheme(), url.host_str().expect("no host"));
    let port = url.port_or_known_default().expect("no port");
    let ollama = Ollama::new_with_client(host, port, http.clone());
    Arc::new(OllamaLLM::new(ollama, model.to_string())) as Arc<dyn LLMClient>
}

#[async_trait]
impl LLMClient for LLMPool {
    async fn chat_stream(
        &self,
        msgs: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.inner.chat_stream(msgs).await
    }
}
