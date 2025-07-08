use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::sync::Mutex;
use tracing::{info, warn};

use psyche_rs::{AbortGuard, ClusterAnalyzer, LLMClient, MemoryStore};

use crate::memory_consolidation_sensor::ConsolidationStatus;

/// Periodically consolidates memories into summaries using [`ClusterAnalyzer`].
///
/// # Example
/// ```no_run
/// use tokio::sync::Mutex;
/// use std::time::Duration;
/// use daringsby::memory_consolidation_service::MemoryConsolidationService;
/// use psyche_rs::{ClusterAnalyzer, InMemoryStore, StoredImpression, MemoryStore};
/// use chrono::Utc;
/// use std::sync::Arc;
/// struct EchoLLM;
/// #[async_trait::async_trait]
/// impl psyche_rs::LLMClient for EchoLLM {
///     async fn chat_stream(
///         &self,
///         _m: &[ollama_rs::generation::chat::ChatMessage],
///     ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         use psyche_rs::Token;
///         let stream = futures::stream::once(async { Token { text: "ok".into() } });
///         Ok(Box::pin(stream))
///     }
///     async fn embed(&self, _t: &str) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(vec![0.0])
///     }
/// }
/// let store = InMemoryStore::new();
/// let llm = Arc::new(EchoLLM);
/// let analyzer = Arc::new(ClusterAnalyzer::new(store, llm));
/// let status = Arc::new(Mutex::new(Default::default()));
/// let service = MemoryConsolidationService::new(analyzer, status, Duration::from_secs(60));
/// let _guard = service.spawn();
/// ```
pub struct MemoryConsolidationService<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> {
    analyzer: Arc<ClusterAnalyzer<M, C>>,
    status: Arc<Mutex<ConsolidationStatus>>,
    batch_size: usize,
    cluster_size: usize,
    interval: Duration,
}

impl<M: MemoryStore + Send + Sync + 'static, C: LLMClient + ?Sized + 'static>
    MemoryConsolidationService<M, C>
{
    /// Create a new service.
    pub fn new(
        analyzer: Arc<ClusterAnalyzer<M, C>>,
        status: Arc<Mutex<ConsolidationStatus>>,
        interval: Duration,
    ) -> Self {
        Self {
            analyzer,
            status,
            batch_size: 20,
            cluster_size: 5,
            interval,
        }
    }

    /// Spawn the consolidation loop in the background.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(self) {
        loop {
            Self::run_once(
                self.analyzer.clone(),
                self.status.clone(),
                self.batch_size,
                self.cluster_size,
            )
            .await;
            tokio::time::sleep(self.interval).await;
        }
    }

    async fn run_once(
        analyzer: Arc<ClusterAnalyzer<M, C>>,
        status: Arc<Mutex<ConsolidationStatus>>,
        batch_size: usize,
        cluster_size: usize,
    ) {
        {
            let mut s = status.lock().await;
            s.in_progress = true;
        }
        let recents = match analyzer.store.fetch_recent_impressions(batch_size).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error=?e, "fetch recent failed");
                status.lock().await.in_progress = false;
                return;
            }
        };
        let mut clusters = Vec::new();
        let mut seen = HashSet::new();
        for imp in &recents {
            if !seen.insert(imp.id.clone()) {
                continue;
            }
            let mut cluster = vec![imp.id.clone()];
            match analyzer
                .store
                .retrieve_related_impressions(&imp.how, cluster_size - 1)
                .await
            {
                Ok(neigh) => {
                    for n in neigh {
                        if seen.insert(n.id.clone()) {
                            cluster.push(n.id);
                        }
                    }
                }
                Err(e) => warn!(error=?e, "neighbor search failed"),
            }
            clusters.push(cluster);
        }
        let count = clusters.len();
        if let Err(e) = analyzer.summarize(clusters).await {
            warn!(error=?e, "summarize failed");
        }
        {
            let mut s = status.lock().await;
            s.in_progress = false;
            s.last_finished = Some(chrono::Utc::now());
            s.cluster_count = count;
        }
        info!("memory consolidation service completed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_consolidation_sensor::ConsolidationStatus;
    use async_trait::async_trait;
    use chrono::Utc;
    use psyche_rs::{InMemoryStore, StoredImpression};

    #[derive(Clone)]
    struct StaticLLM {
        reply: String,
    }

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _m: &[ollama_rs::generation::chat::ChatMessage],
        ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            use psyche_rs::Token;
            let r = self.reply.clone();
            Ok(Box::pin(futures::stream::once(async { Token { text: r } })))
        }
        async fn embed(
            &self,
            _t: &str,
        ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![0.0])
        }
    }

    #[tokio::test]
    async fn summarizes_in_background() {
        let store = Arc::new(InMemoryStore::new());
        let llm = Arc::new(StaticLLM {
            reply: "sum".into(),
        });
        let analyzer = Arc::new(ClusterAnalyzer::new(store.clone(), llm));
        let status = Arc::new(Mutex::new(ConsolidationStatus::default()));

        let imp1 = StoredImpression {
            id: "i1".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "a".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        let imp2 = StoredImpression {
            id: "i2".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "b".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        store.store_impression(&imp1).await.unwrap();
        store.store_impression(&imp2).await.unwrap();

        let service =
            MemoryConsolidationService::new(analyzer, status.clone(), Duration::from_millis(50));
        let guard = service.spawn();
        tokio::time::sleep(Duration::from_millis(120)).await;
        drop(guard);
        let imps = store.fetch_recent_impressions(10).await.unwrap();
        // At least one summary should exist alongside originals
        assert!(imps.len() >= 3);
        let s = status.lock().await;
        assert!(s.last_finished.is_some());
        assert_eq!(s.cluster_count, 1);
    }
}
