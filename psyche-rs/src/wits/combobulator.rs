use std::collections::VecDeque;
use std::sync::Arc;

use uuid::Uuid;

use crate::llm::LLMExt;
use crate::memory::{Impression, Memory, MemoryStore};
use crate::wit::Wit;
use llm::chat::ChatProvider;

/// `Combobulator` aggregates instant impressions into higher level temporal
/// memories such as "moments" or "situations".
///
/// It collects a short window of instant impressions and asks the provided
/// [`ChatProvider`] for a one-sentence summary. The resulting [`Impression`] is
/// stored via the [`MemoryStore`].
pub struct Combobulator {
    buffer: VecDeque<Impression>,
    /// Topic label for produced impressions (e.g. `"moment"` or `"situation"`).
    topic: String,
    store: Arc<dyn MemoryStore>,
    llm: Arc<dyn ChatProvider>,
}

impl Combobulator {
    /// Create a new `Combobulator` producing impressions of the given `topic`.
    pub fn new(topic: &str, store: Arc<dyn MemoryStore>, llm: Arc<dyn ChatProvider>) -> Self {
        Self {
            buffer: VecDeque::new(),
            topic: topic.into(),
            store,
            llm,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Wit<Impression, Impression> for Combobulator {
    /// Observe an instant impression. Other topics are ignored.
    async fn observe(&mut self, input: Impression) {
        if input.topic == "instant" {
            self.buffer.push_back(input);
            if self.buffer.len() > 5 {
                self.buffer.pop_front();
            }
        }
    }

    /// Summarise buffered instants into a higher-level impression.
    async fn distill(&mut self) -> Option<Impression> {
        if self.buffer.len() < 3 {
            return None;
        }

        let impressions: Vec<_> = self.buffer.drain(..).collect();
        let summary = self.llm.summarize_impressions(&impressions).await.ok()?;
        let ids = impressions.iter().map(|i| i.uuid).collect::<Vec<_>>();
        let timestamp = impressions.last()?.timestamp;

        let imp = Impression {
            uuid: Uuid::new_v4(),
            how: summary,
            topic: self.topic.clone(),
            composed_of: ids,
            timestamp,
        };

        let _ = self.store.save(&Memory::Impression(imp.clone())).await;
        Some(imp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Memory, MemoryStore};
    use llm::chat::{ChatProvider, ChatResponse};
    use std::collections::HashMap;
    use std::time::SystemTime;
    use tokio::sync::Mutex as AsyncMutex;

    struct MockStore {
        data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                data: Arc::new(AsyncMutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl MemoryStore for MockStore {
        async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
            self.data.lock().await.insert(memory.uuid(), memory.clone());
            Ok(())
        }
        async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
            Ok(self.data.lock().await.get(&uuid).cloned())
        }
        async fn recent(&self, _l: usize) -> anyhow::Result<Vec<Memory>> {
            Ok(Vec::new())
        }
        async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
            Ok(Vec::new())
        }
        async fn recent_since(&self, _s: SystemTime) -> anyhow::Result<Vec<Memory>> {
            Ok(Vec::new())
        }
        async fn impressions_containing(&self, _k: &str) -> anyhow::Result<Vec<Impression>> {
            Ok(Vec::new())
        }
        async fn complete_intention(&self, _i: Uuid, _c: crate::Completion) -> anyhow::Result<()> {
            Ok(())
        }
        async fn interrupt_intention(
            &self,
            _i: Uuid,
            _c: crate::Interruption,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct SimpleResp(String);
    impl ChatResponse for SimpleResp {
        fn text(&self) -> Option<String> {
            Some(self.0.clone())
        }
        fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
            None
        }
    }
    impl std::fmt::Display for SimpleResp {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    struct MockLLM;

    #[async_trait::async_trait]
    impl ChatProvider for MockLLM {
        async fn chat_with_tools(
            &self,
            _messages: &[llm::chat::ChatMessage],
            _tools: Option<&[llm::chat::Tool]>,
        ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
            Ok(Box::new(SimpleResp("".into())))
        }

        async fn chat_stream(
            &self,
            messages: &[llm::chat::ChatMessage],
        ) -> Result<
            std::pin::Pin<
                Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
            >,
            llm::error::LLMError,
        > {
            let reply = format!(
                "{} combined",
                messages.last().unwrap().content.lines().count() - 1
            );
            Ok(Box::pin(futures_util::stream::once(
                async move { Ok(reply) },
            )))
        }
    }

    fn sample_instant() -> Impression {
        Impression {
            uuid: Uuid::new_v4(),
            how: "hi".into(),
            topic: "instant".into(),
            composed_of: Vec::new(),
            timestamp: SystemTime::now(),
        }
    }

    #[tokio::test]
    async fn produces_summary_when_enough_instants() {
        let store = Arc::new(MockStore::new());
        let llm = Arc::new(MockLLM);
        let mut combo = Combobulator::new("moment", store.clone(), llm);

        let i1 = sample_instant();
        let i2 = sample_instant();
        let i3 = sample_instant();

        combo.observe(i1.clone()).await;
        combo.observe(i2.clone()).await;
        combo.observe(i3.clone()).await;

        let imp = combo.distill().await.expect("no impression");
        assert_eq!(imp.topic, "moment");
        assert_eq!(imp.composed_of, vec![i1.uuid, i2.uuid, i3.uuid]);

        let saved = store.get_by_uuid(imp.uuid).await.unwrap();
        assert!(matches!(saved, Some(Memory::Impression(_))));
    }
}
