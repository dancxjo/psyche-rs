use std::collections::VecDeque;
use std::sync::Arc;

use uuid::Uuid;

use crate::llm::LLMClient;
use crate::memory::{Impression, Memory, MemoryStore, Sensation};
use crate::wit::Wit;

/// `Quick` buffers recent [`Sensation`]s and periodically summarizes them into
/// [`Impression`]s representing short instants.
///
/// The summarization logic is intentionally simple for now, producing a single
/// sentence describing how many sensations were observed.
pub struct Quick {
    buffer: VecDeque<Sensation>,
    store: Arc<dyn MemoryStore>,
    llm: Arc<dyn LLMClient>,
}

impl Quick {
    /// Create a new [`Quick`] wit using the given [`MemoryStore`].
    pub fn new(store: Arc<dyn MemoryStore>, llm: Arc<dyn LLMClient>) -> Self {
        Self {
            buffer: VecDeque::new(),
            store,
            llm,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Wit<Sensation, Impression> for Quick {
    /// Push the sensation onto the internal buffer, keeping at most the latest
    /// ten entries.
    async fn observe(&mut self, input: Sensation) {
        self.buffer.push_back(input);
        if self.buffer.len() > 10 {
            self.buffer.pop_front();
        }
    }

    /// Summarise the buffered sensations into a single [`Impression`]. Returns
    /// `None` if no sensations have been observed since the last call.
    async fn distill(&mut self) -> Option<Impression> {
        if self.buffer.is_empty() {
            return None;
        }

        // Generate a natural language summary using the provided LLM. Any
        // failure to obtain a summary results in no impression being produced.
        let sensations: Vec<_> = self.buffer.iter().cloned().collect();
        let summary = self.llm.summarize(&sensations).await.ok()?;
        let ids = self.buffer.iter().map(|s| s.uuid).collect::<Vec<_>>();
        let timestamp = self.buffer.back().unwrap().timestamp;

        let impression = Impression {
            uuid: Uuid::new_v4(),
            how: summary,
            topic: "instant".into(),
            composed_of: ids,
            timestamp,
        };

        // Persist the impression, ignoring any store errors for now.
        let _ = self
            .store
            .save(&Memory::Impression(impression.clone()))
            .await;

        // Optionally generate urges from the LLM and store them.
        if let Ok(urges) = self.llm.suggest_urges(&impression).await {
            for urge in urges {
                let _ = self.store.save(&Memory::Urge(urge)).await;
            }
        }

        self.buffer.clear();
        Some(impression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryStore, Urge};
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::SystemTime;
    use tokio::sync::Mutex as AsyncMutex;

    /// Simple in-memory store used for wit tests.
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

        async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
            Ok(Vec::new())
        }

        async fn of_type(&self, _type: &str, _limit: usize) -> anyhow::Result<Vec<Memory>> {
            Ok(Vec::new())
        }

        async fn complete_intention(
            &self,
            _intention_id: Uuid,
            _completion: crate::memory::Completion,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn interrupt_intention(
            &self,
            _intention_id: Uuid,
            _interruption: crate::memory::Interruption,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn sample_sensation() -> Sensation {
        Sensation {
            uuid: Uuid::new_v4(),
            kind: "test".into(),
            from: "tester".into(),
            payload: json!({"x": 1}),
            timestamp: SystemTime::now(),
        }
    }

    #[tokio::test]
    async fn distills_buffer_into_impression() {
        let store = Arc::new(MockStore::new());
        let llm = Arc::new(crate::llm::DummyLLM);
        let mut quick = Quick::new(store.clone(), llm);

        let s1 = sample_sensation();
        let s2 = sample_sensation();

        quick.observe(s1.clone()).await;
        quick.observe(s2.clone()).await;

        let imp = quick.distill().await.expect("should produce impression");
        assert_eq!(imp.topic, "instant");
        assert_eq!(imp.composed_of, vec![s1.uuid, s2.uuid]);

        let saved = store.get_by_uuid(imp.uuid).await.unwrap();
        assert!(matches!(saved, Some(Memory::Impression(_))));
    }

    #[tokio::test]
    async fn keeps_only_last_ten_observations() {
        let store = Arc::new(MockStore::new());
        let llm = Arc::new(crate::llm::DummyLLM);
        let mut quick = Quick::new(store.clone(), llm);

        let mut uuids = Vec::new();
        for _ in 0..11 {
            let s = sample_sensation();
            uuids.push(s.uuid);
            quick.observe(s).await;
        }

        let imp = quick.distill().await.unwrap();
        // Only last 10 should remain
        assert_eq!(imp.composed_of.len(), 10);
        assert_eq!(imp.composed_of.first().copied(), Some(uuids[1]));
    }

    struct MockLLM {
        summaries: Arc<AsyncMutex<usize>>,
        urges: Arc<AsyncMutex<usize>>,
    }

    impl MockLLM {
        fn new() -> Self {
            Self {
                summaries: Arc::new(AsyncMutex::new(0)),
                urges: Arc::new(AsyncMutex::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::llm::LLMClient for MockLLM {
        async fn summarize(&self, input: &[Sensation]) -> anyhow::Result<String> {
            *self.summaries.lock().await += 1;
            Ok(format!("{} sensed", input.len()))
        }

        async fn summarize_impressions(&self, _items: &[Impression]) -> anyhow::Result<String> {
            Ok("summary".into())
        }

        async fn suggest_urges(&self, impression: &Impression) -> anyhow::Result<Vec<Urge>> {
            *self.urges.lock().await += 1;
            Ok(vec![Urge {
                uuid: Uuid::new_v4(),
                source: impression.uuid,
                motor_name: "mock".into(),
                parameters: serde_json::json!({}),
                intensity: 1.0,
                timestamp: impression.timestamp,
            }])
        }

        async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
            Ok("I feel nothing".into())
        }
    }

    #[tokio::test]
    async fn llm_summary_and_urge_saved() {
        let store = Arc::new(MockStore::new());
        let llm = Arc::new(MockLLM::new());
        let mut quick = Quick::new(store.clone(), llm.clone());

        quick.observe(sample_sensation()).await;

        let imp = quick.distill().await.unwrap();

        assert_eq!(*llm.summaries.lock().await, 1);
        assert_eq!(*llm.urges.lock().await, 1);

        let saved_imp = store.get_by_uuid(imp.uuid).await.unwrap();
        assert!(matches!(saved_imp, Some(Memory::Impression(_))));

        let urge_count = store
            .data
            .lock()
            .await
            .values()
            .filter(|m| matches!(m, Memory::Urge(_)))
            .count();
        assert_eq!(urge_count, 1);
    }
}
