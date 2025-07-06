use async_trait::async_trait;
use futures::StreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ollama_rs::generation::chat::ChatMessage;
use psyche_rs::{
    LLMClient, MemoryStore, RetryLLM, RetryMemoryStore, RetryPolicy, StoredImpression,
    StoredSensation,
};

struct FlakyLLM {
    fails: AtomicUsize,
}

#[async_trait]
impl LLMClient for FlakyLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ChatMessage],
    ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        if self.fails.fetch_sub(1, Ordering::SeqCst) > 0 {
            Err("fail".into())
        } else {
            use psyche_rs::Token;
            Ok(Box::pin(futures::stream::once(async {
                Token { text: "ok".into() }
            })))
        }
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![0.0])
    }
}

#[derive(Default)]
struct FlakyStore {
    fails: AtomicUsize,
    inner: psyche_rs::InMemoryStore,
}

#[async_trait]
impl MemoryStore for FlakyStore {
    async fn store_sensation(&self, s: &StoredSensation) -> anyhow::Result<()> {
        if self.fails.fetch_sub(1, Ordering::SeqCst) > 0 {
            Err(anyhow::anyhow!("fail"))
        } else {
            self.inner.store_sensation(s).await
        }
    }

    async fn store_impression(&self, i: &StoredImpression) -> anyhow::Result<()> {
        self.inner.store_impression(i).await
    }

    async fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        self.inner
            .add_lifecycle_stage(impression_id, stage, detail)
            .await
    }

    async fn retrieve_related_impressions(
        &self,
        query: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        self.inner.retrieve_related_impressions(query, top_k).await
    }

    async fn fetch_recent_impressions(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        self.inner.fetch_recent_impressions(limit).await
    }

    async fn load_full_impression(
        &self,
        id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        std::collections::HashMap<String, String>,
    )> {
        self.inner.load_full_impression(id).await
    }
}

#[tokio::test]
async fn retries_llm_until_success() {
    let llm = FlakyLLM {
        fails: AtomicUsize::new(2),
    };
    let policy = RetryPolicy::new(3, Duration::from_millis(1));
    let client = RetryLLM::new(llm, policy);
    let msgs = [ChatMessage::user("hi".to_string())];
    let mut stream = client.chat_stream(&msgs).await.unwrap();
    let tok = stream.next().await.unwrap();
    assert_eq!(tok.text, "ok");
}

#[tokio::test]
async fn retries_memory_store_until_success() {
    let store = FlakyStore {
        fails: AtomicUsize::new(1),
        inner: psyche_rs::InMemoryStore::new(),
    };
    let policy = RetryPolicy::new(2, Duration::from_millis(1));
    let retry_store = RetryMemoryStore::new(store, policy);
    let s = StoredSensation {
        id: "s".into(),
        kind: "t".into(),
        when: chrono::Utc::now(),
        data: "{}".into(),
    };
    retry_store.store_sensation(&s).await.unwrap();
}
