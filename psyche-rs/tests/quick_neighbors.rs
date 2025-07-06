use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use psyche_rs::{
    Genius, InstantInput, MemoryStore, QuickGenius, StoredImpression, StoredSensation,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct SpyStore {
    queries: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl MemoryStore for SpyStore {
    async fn store_sensation(&self, _s: &StoredSensation) -> anyhow::Result<()> {
        Ok(())
    }
    async fn store_impression(&self, _i: &StoredImpression) -> anyhow::Result<()> {
        Ok(())
    }
    async fn add_lifecycle_stage(&self, _i: &str, _s: &str, _d: &str) -> anyhow::Result<()> {
        Ok(())
    }
    async fn retrieve_related_impressions(
        &self,
        how: &str,
        _k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        self.queries.lock().unwrap().push(how.to_string());
        Ok(vec![StoredImpression {
            id: "n".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "m".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        }])
    }
    async fn fetch_recent_impressions(&self, _l: usize) -> anyhow::Result<Vec<StoredImpression>> {
        Ok(Vec::new())
    }
    async fn load_full_impression(
        &self,
        _id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        HashMap<String, String>,
    )> {
        Err(anyhow::anyhow!("no"))
    }
}

#[tokio::test]
async fn queries_memory_for_neighbors() {
    let queries = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::new(SpyStore {
        queries: queries.clone(),
    });
    let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
    let rx = tokio::sync::mpsc::channel(1).1;
    let genius = QuickGenius::new(rx, out_tx).memory_store(store);
    let mut stream = genius
        .call(InstantInput {
            description: "hello".into(),
        })
        .await;
    while stream.next().await.is_some() {}
    let q = queries.lock().unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0], "hello");
}
