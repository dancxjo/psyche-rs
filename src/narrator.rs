use std::sync::Arc;
use std::time::SystemTime;

use crate::llm::LLMClient;
use crate::memory::{Memory, MemoryStore};
use crate::store::MemoryRetriever;

/// `Narrator` provides high level summaries of Pete's memories by
/// querying a [`MemoryStore`] and condensing results using an
/// [`LLMClient`].
///
/// ```no_run
/// # use psyche_rs::{Narrator, memory::{MemoryStore, Memory}, llm::LLMClient};
/// # use std::sync::Arc;
/// # use std::time::SystemTime;
/// # use async_trait::async_trait;
/// # struct DummyStore;
/// # #[async_trait]
/// # impl MemoryStore for DummyStore {
/// #     async fn save(&self, _: &Memory) -> anyhow::Result<()> { Ok(()) }
/// #     async fn get_by_uuid(&self, _: uuid::Uuid) -> anyhow::Result<Option<Memory>> { Ok(None) }
/// #     async fn recent(&self, _: usize) -> anyhow::Result<Vec<Memory>> { Ok(vec![]) }
/// #     async fn of_type(&self, _: &str, _: usize) -> anyhow::Result<Vec<Memory>> { Ok(vec![]) }
/// #     async fn complete_intention(&self, _: uuid::Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> { Ok(()) }
/// #     async fn interrupt_intention(&self, _: uuid::Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> { Ok(()) }
/// #     async fn recent_since(&self, _: std::time::SystemTime) -> anyhow::Result<Vec<Memory>> { Ok(vec![]) }
/// #     async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<psyche_rs::Impression>> { Ok(vec![]) }
/// # }
/// # struct DummyLLM;
/// # #[async_trait]
/// # impl LLMClient for DummyLLM {
/// #     async fn summarize(&self, _: &[psyche_rs::Sensation]) -> anyhow::Result<String> { Ok(String::new()) }
/// #     async fn summarize_impressions(&self, _: &[psyche_rs::Impression]) -> anyhow::Result<String> { Ok(String::new()) }
/// #     async fn suggest_urges(&self, _: &psyche_rs::Impression) -> anyhow::Result<Vec<psyche_rs::Urge>> { Ok(vec![]) }
/// #     async fn evaluate_emotion(&self, _: &Memory) -> anyhow::Result<String> { Ok(String::new()) }
/// # }
/// # async fn demo() -> anyhow::Result<()> {
/// # let store = Arc::new(DummyStore);
/// # let llm = Arc::new(DummyLLM);
/// # let retriever = Arc::new(psyche_rs::store::embedding_store::NoopRetriever);
/// let narrator = Narrator { store, llm, retriever };
/// let _story = narrator.narrate_since(SystemTime::now()).await?;
/// # Ok(()) }
/// ```
#[derive(Clone)]
pub struct Narrator {
    pub store: Arc<dyn MemoryStore>,
    pub llm: Arc<dyn LLMClient>,
    /// Embedding-based retriever used for memory search.
    pub retriever: Arc<dyn MemoryRetriever>,
}

impl Narrator {
    /// Summarize all impressions recorded since the given time.
    pub async fn narrate_since(&self, since: SystemTime) -> anyhow::Result<String> {
        let memories = self.store.recent_since(since).await?;
        let impressions: Vec<_> = memories
            .into_iter()
            .filter_map(|m| match m {
                Memory::Impression(i) => Some(i),
                _ => None,
            })
            .collect();
        let story = self.llm.summarize_impressions(&impressions).await?;
        Ok(story)
    }

    /// Summarize impressions containing the given keyword.
    pub async fn narrate_topic(&self, keyword: &str) -> anyhow::Result<String> {
        let impressions = self.store.impressions_containing(keyword).await?;
        let story = self.llm.summarize_impressions(&impressions).await?;
        Ok(story)
    }

    /// Retrieve impressions relevant to the provided `text` using the
    /// configured [`MemoryRetriever`]. The returned impressions are
    /// summarised into a short story by the [`LLMClient`].
    pub async fn recall_relevant(&self, text: &str) -> anyhow::Result<String> {
        // look up nearest memories via embeddings
        // Fetch the single most relevant memory for now.
        let ids = self.retriever.find_similar(text, 1).await?;
        let mut impressions = Vec::new();
        for id in ids {
            if let Some(Memory::Impression(i)) = self.store.get_by_uuid(id).await? {
                impressions.push(i);
            }
        }
        let story = self.llm.summarize_impressions(&impressions).await?;
        Ok(story)
    }
}
