use psyche_rs::{
    llm::DummyLLM,
    memory::{Memory, MemoryStore, Sensation},
    wit::Wit,
    wits::quick::Quick,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

/// Dummy in-memory store used for streaming tests.
struct DummyMemoryStore {
    data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
}

impl DummyMemoryStore {
    fn new() -> Self {
        Self {
            data: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for DummyMemoryStore {
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

    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn recent_since(&self, _: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<psyche_rs::Impression>> {
        Ok(Vec::new())
    }

    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

fn make_fake_sensation(n: usize) -> Sensation {
    Sensation {
        uuid: Uuid::new_v4(),
        kind: "text/plain".into(),
        from: "test".into(),
        payload: json!({ "content": format!("event_{}", n) }),
        timestamp: SystemTime::now(),
    }
}

#[tokio::test]
async fn quick_summarizes_sensations_stream() {
    let store = Arc::new(DummyMemoryStore::new());
    let mut quick = Quick::new(store.clone(), Arc::new(DummyLLM));

    for i in 0..15 {
        quick.observe(make_fake_sensation(i)).await;
    }

    let imp = quick.distill().await.expect("Quick failed to summarize");
    println!("Impression: {:?}", imp);

    let saved = store.get_by_uuid(imp.uuid).await.unwrap();
    assert!(matches!(saved, Some(Memory::Impression(_))));
}
