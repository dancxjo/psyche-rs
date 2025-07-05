use std::sync::Arc;

use crate::{LLMClient, MemoryStore};

/// Context scoped to a single thread.
///
/// Each worker thread should create its own [`ThreadLocalContext`] containing
/// dedicated instances of external clients. This avoids cross-thread
/// contention and makes it easier to manage per-thread resources.
///
/// ```
/// use psyche_rs::{ThreadLocalContext, InMemoryStore, LLMClient};
/// use std::sync::Arc;
///
/// #[derive(Clone)]
/// struct StubLLM;
///
/// #[async_trait::async_trait]
/// impl LLMClient for StubLLM {
///     async fn chat_stream(
///         &self,
///         _msgs: &[ollama_rs::generation::chat::ChatMessage],
///     ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(Box::pin(futures::stream::empty()))
///     }
///     async fn embed(
///         &self,
///         _text: &str,
///     ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(Vec::new())
///     }
/// }
///
/// let ctx = ThreadLocalContext {
///     llm: Arc::new(StubLLM),
///     store: Arc::new(InMemoryStore::new()),
/// };
/// ```
#[derive(Clone)]
pub struct ThreadLocalContext {
    /// LLM client used exclusively by this thread.
    pub llm: Arc<dyn LLMClient>,
    /// Memory store instance scoped to this thread.
    pub store: Arc<dyn MemoryStore + Send + Sync>,
}
