use crate::llm::types::TokenStream;
use crate::{LLMClient, MemoryStore};
use async_trait::async_trait;
use ollama_rs::generation::chat::ChatMessage;
use std::time::Duration;

/// Policy controlling how many times an operation is retried and the delay
/// between attempts.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of additional attempts to make after the first failure.
    pub max_retries: usize,
    /// Delay between retry attempts.
    pub delay: Duration,
}

impl RetryPolicy {
    /// Create a new policy.
    pub fn new(max_retries: usize, delay: Duration) -> Self {
        Self { max_retries, delay }
    }

    /// Execute `op` retrying on error according to the policy.
    pub async fn retry<F, Fut, T, E>(&self, mut op: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let mut attempts = 0usize;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(_e) if attempts < self.max_retries => {
                    attempts += 1;
                    tokio::time::sleep(self.delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Wrapper around an [`LLMClient`] that applies a [`RetryPolicy`].
#[derive(Clone)]
pub struct RetryLLM<C> {
    inner: C,
    policy: RetryPolicy,
}

impl<C> RetryLLM<C> {
    /// Construct a new retrying LLM client.
    pub fn new(inner: C, policy: RetryPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl<C> LLMClient for RetryLLM<C>
where
    C: LLMClient + Send + Sync,
{
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.policy.retry(|| self.inner.chat_stream(messages)).await
    }

    async fn embed(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        self.policy.retry(|| self.inner.embed(text)).await
    }
}

/// Wrapper around a [`MemoryStore`] that retries failed operations.
pub struct RetryMemoryStore<M> {
    inner: M,
    policy: RetryPolicy,
}

impl<M> RetryMemoryStore<M> {
    /// Wrap a memory store with retry behaviour.
    pub fn new(inner: M, policy: RetryPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl<M> MemoryStore for RetryMemoryStore<M>
where
    M: MemoryStore + Send + Sync,
{
    async fn store_sensation(
        &self,
        s: &crate::memory_store::StoredSensation,
    ) -> anyhow::Result<()> {
        self.policy.retry(|| self.inner.store_sensation(s)).await
    }

    async fn find_sensation(
        &self,
        kind: &str,
        data: &str,
    ) -> anyhow::Result<Option<crate::memory_store::StoredSensation>> {
        self.policy
            .retry(|| self.inner.find_sensation(kind, data))
            .await
    }

    async fn store_impression(
        &self,
        i: &crate::memory_store::StoredImpression,
    ) -> anyhow::Result<()> {
        self.policy.retry(|| self.inner.store_impression(i)).await
    }

    async fn store_summary_impression(
        &self,
        summary: &crate::memory_store::StoredImpression,
        linked_ids: &[String],
    ) -> anyhow::Result<()> {
        self.policy
            .retry(|| self.inner.store_summary_impression(summary, linked_ids))
            .await
    }

    async fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        self.policy
            .retry(|| self.inner.add_lifecycle_stage(impression_id, stage, detail))
            .await
    }

    async fn retrieve_related_impressions(
        &self,
        query_how: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<crate::memory_store::StoredImpression>> {
        self.policy
            .retry(|| self.inner.retrieve_related_impressions(query_how, top_k))
            .await
    }

    async fn fetch_recent_impressions(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<crate::memory_store::StoredImpression>> {
        self.policy
            .retry(|| self.inner.fetch_recent_impressions(limit))
            .await
    }

    async fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        crate::memory_store::StoredImpression,
        Vec<crate::memory_store::StoredSensation>,
        std::collections::HashMap<String, String>,
    )> {
        self.policy
            .retry(|| self.inner.load_full_impression(impression_id))
            .await
    }
}
