use std::sync::Arc;
use std::time::SystemTime;

use crate::llm::LLMExt;
use crate::memory::{Memory, MemoryStore};
use crate::store::MemoryRetriever;
use llm::chat::ChatProvider;

/// `Narrator` provides high level summaries of Pete's memories by
/// querying a [`MemoryStore`] and condensing results using an
/// [`ChatProvider`].
///
/// ```no_run
/// # use psyche_rs::{Narrator, memory::{MemoryStore, Memory}, llm::LLMExt};
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
/// # impl llm::chat::ChatProvider for DummyLLM {
/// #     async fn chat_with_tools(&self, _: &[llm::chat::ChatMessage], _: Option<&[llm::chat::Tool]>) -> Result<Box<dyn llm::chat::ChatResponse>, llm::error::LLMError> { Ok(Box::new(SimpleResp(String::new()))) }
/// #     async fn chat_stream(&self, _: &[llm::chat::ChatMessage]) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>>, llm::error::LLMError> { Ok(Box::pin(futures_util::stream::empty())) }
/// # }
/// # #[derive(Debug)]
/// # struct SimpleResp(String);
/// # impl llm::chat::ChatResponse for SimpleResp { fn text(&self) -> Option<String> { Some(self.0.clone()) } fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> { None } }
/// # impl std::fmt::Display for SimpleResp { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) } }
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
    pub llm: Arc<dyn ChatProvider>,
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
    /// summarised into a short story by the [`ChatProvider`].
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
