use std::collections::VecDeque;
use std::sync::Arc;

use uuid::Uuid;

use crate::llm::LLMClient;
use crate::memory::{Impression, Memory, MemoryStore};
use crate::wit::Wit;

/// `Combobulator` aggregates instant impressions into higher level temporal
/// memories such as "moments" or "situations".
///
/// It collects a short window of instant impressions and asks the provided
/// [`LLMClient`] for a one-sentence summary. The resulting [`Impression`] is
/// stored via the [`MemoryStore`].
pub struct Combobulator {
    buffer: VecDeque<Impression>,
    /// Topic label for produced impressions (e.g. `"moment"` or `"situation"`).
    topic: String,
    store: Arc<dyn MemoryStore>,
    llm: Arc<dyn LLMClient>,
}

impl Combobulator {
    /// Create a new `Combobulator` producing impressions of the given `topic`.
    pub fn new(topic: &str, store: Arc<dyn MemoryStore>, llm: Arc<dyn LLMClient>) -> Self {
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
    use crate::llm::LLMClient;
    use crate::memory::{Memory, MemoryStore};
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

    struct MockLLM;

    #[async_trait::async_trait]
    impl LLMClient for MockLLM {
        async fn summarize(&self, _input: &[crate::Sensation]) -> anyhow::Result<String> {
            Ok("".into())
        }
        async fn suggest_urges(&self, _imp: &Impression) -> anyhow::Result<Vec<crate::Urge>> {
            Ok(vec![])
        }
        async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
            Ok("".into())
        }
        async fn summarize_impressions(&self, items: &[Impression]) -> anyhow::Result<String> {
            Ok(format!("{} combined", items.len()))
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
