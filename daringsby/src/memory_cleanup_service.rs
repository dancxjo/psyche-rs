use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tracing::info;

use psyche_rs::{AbortGuard, ClusterAnalyzer, LLMClient, MemoryStore};

/// Periodically prompts the LLM to prune old memories. If the summarizer
/// responds with `{{DELETE}}` the corresponding impressions are removed via
/// [`MemoryStore::delete_impression`].
pub struct MemoryCleanupService<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> {
    analyzer: Arc<ClusterAnalyzer<M, C>>,
    batch_size: usize,
    interval: Duration,
}

impl<M: MemoryStore + Send + Sync + 'static, C: LLMClient + ?Sized + 'static>
    MemoryCleanupService<M, C>
{
    /// Create a new cleanup service.
    pub fn new(analyzer: Arc<ClusterAnalyzer<M, C>>, interval: Duration) -> Self {
        Self {
            analyzer,
            batch_size: 20,
            interval,
        }
    }

    /// Spawn the cleanup loop in the background.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(self) {
        loop {
            Self::run_once(self.analyzer.clone(), self.batch_size).await;
            sleep(self.interval).await;
        }
    }

    async fn run_once(analyzer: Arc<ClusterAnalyzer<M, C>>, batch_size: usize) {
        let recents = match analyzer.store.fetch_recent_impressions(batch_size).await {
            Ok(v) => v,
            Err(_) => return,
        };
        let clusters: Vec<Vec<String>> = recents.into_iter().map(|i| vec![i.id]).collect();
        let _ = analyzer.summarize(clusters).await;
        info!("memory cleanup service completed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use psyche_rs::{InMemoryStore, StoredImpression};

    #[derive(Clone)]
    struct StaticLLM;

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _m: &[ollama_rs::generation::chat::ChatMessage],
        ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            use psyche_rs::Token;
            Ok(Box::pin(futures::stream::once(async {
                Token {
                    text: "{{DELETE}}".into(),
                }
            })))
        }

        async fn embed(
            &self,
            _t: &str,
        ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![0.0])
        }
    }

    #[tokio::test]
    async fn cleans_up_in_background() {
        let store = Arc::new(InMemoryStore::new());
        let llm = Arc::new(StaticLLM);
        let analyzer = Arc::new(ClusterAnalyzer::new(store.clone(), llm));

        let imp = StoredImpression {
            id: "c1".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "tmp".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        store.store_impression(&imp).await.unwrap();

        let svc = MemoryCleanupService::new(analyzer, Duration::from_millis(50));
        let guard = svc.spawn();
        tokio::time::sleep(Duration::from_millis(120)).await;
        drop(guard);

        assert_eq!(store.fetch_recent_impressions(10).await.unwrap().len(), 0);
    }
}
