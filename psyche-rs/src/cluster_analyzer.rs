use crate::llm::types::Token;
use crate::{
    LLMClient,
    memory_store::{MemoryStore, StoredImpression},
};
use chrono::Utc;
use futures::StreamExt;
use ollama_rs::generation::chat::ChatMessage;
use std::sync::Arc;

/// Analyzes clusters of impressions and stores one-sentence summaries.
///
/// # Example
/// ```
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
///         let stream = futures::stream::once(async { Token { text: "echo".into() } });
///         Ok(Box::pin(stream))
///     }
///     async fn embed(
///         &self,
///         _t: &str,
///     ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(vec![0.0])
///     }
/// }
/// let store = InMemoryStore::new();
/// let llm = Arc::new(EchoLLM);
/// let analyzer = ClusterAnalyzer::new(store, llm);
/// let imp = StoredImpression {
///     id: "i1".into(),
///     kind: "Instant".into(),
///     when: Utc::now(),
///     how: "hi".into(),
///     sensation_ids: Vec::new(),
///     impression_ids: Vec::new(),
/// };
/// tokio_test::block_on(async {
///     analyzer.store.store_impression(&imp).await.unwrap();
///     analyzer.summarize(vec![vec!["i1".to_string()]]).await.unwrap();
/// });
/// ```
pub struct ClusterAnalyzer<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> {
    pub store: M,
    llm: Arc<C>,
}

impl<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> ClusterAnalyzer<M, C> {
    /// Create a new analyzer.
    pub fn new(store: M, llm: Arc<C>) -> Self {
        Self { store, llm }
    }

    /// Summarize each cluster of impression IDs.
    pub async fn summarize(
        &self,
        clusters: Vec<Vec<String>>,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        let mut summaries = Vec::new();
        for cluster in clusters {
            if cluster.is_empty() {
                continue;
            }
            let mut sentences = Vec::new();
            for id in &cluster {
                let (imp, _, _) = self.store.load_full_impression(id).await?;
                sentences.push(imp.how);
            }
            let prompt = format!(
                "Summarize the following related memories into one natural sentence that best describes their common theme. If they are irrelevant, repetitive, or incorrect, reply with {{DELETE}} instead:\n{}",
                sentences.join("\n")
            );
            tracing::trace!(?prompt, "summary_prompt");
            let messages = [ChatMessage::user(prompt)];
            let mut stream = self
                .llm
                .chat_stream(&messages)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            let mut out = String::new();
            while let Some(Token { text }) = stream.next().await {
                tracing::trace!(token = %text, "summary_token");
                out.push_str(&text);
            }
            tracing::debug!(%out, "llm full response");
            let out_trim = out.trim();
            if out_trim == "{{DELETE}}" {
                for id in &cluster {
                    self.store.delete_impression(id).await?;
                }
            } else {
                let summary = StoredImpression {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: "Summary".into(),
                    when: Utc::now(),
                    how: out_trim.to_string(),
                    sensation_ids: Vec::new(),
                    impression_ids: cluster.clone(),
                };
                self.store
                    .store_summary_impression(&summary, &cluster)
                    .await?;
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_store::InMemoryStore;
    use async_trait::async_trait;
    use futures::stream;

    #[derive(Clone)]
    struct StaticLLM {
        reply: String,
    }

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _m: &[ChatMessage],
        ) -> Result<
            crate::llm::types::TokenStream,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            let reply = self.reply.clone();
            use crate::llm::types::Token;
            Ok(Box::pin(stream::once(async move { Token { text: reply } })))
        }

        async fn embed(
            &self,
            _text: &str,
        ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![0.0])
        }
    }

    #[tokio::test]
    async fn stores_summary_impression() {
        let store = InMemoryStore::new();
        let imp1 = StoredImpression {
            id: "i1".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "hello".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        let imp2 = StoredImpression {
            id: "i2".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "world".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        store.store_impression(&imp1).await.unwrap();
        store.store_impression(&imp2).await.unwrap();

        let llm = Arc::new(StaticLLM {
            reply: "summary".into(),
        });
        let analyzer = ClusterAnalyzer::new(store, llm);
        let sums = analyzer
            .summarize(vec![vec!["i1".into(), "i2".into()]])
            .await
            .unwrap();
        assert_eq!(sums.len(), 1);
        let sum = &sums[0];
        assert_eq!(sum.kind, "Summary");
        assert_eq!(sum.impression_ids, vec!["i1", "i2"]);
    }

    #[tokio::test]
    async fn deletes_cluster_when_llm_requests() {
        let store = InMemoryStore::new();
        let imp = StoredImpression {
            id: "del1".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "noise".into(),
            sensation_ids: Vec::new(),
            impression_ids: Vec::new(),
        };
        store.store_impression(&imp).await.unwrap();

        let llm = Arc::new(StaticLLM {
            reply: "{{DELETE}}".into(),
        });
        let analyzer = ClusterAnalyzer::new(store, llm);
        analyzer.summarize(vec![vec!["del1".into()]]).await.unwrap();
        assert_eq!(
            analyzer
                .store
                .fetch_recent_impressions(1)
                .await
                .unwrap()
                .len(),
            0
        );
    }
}
