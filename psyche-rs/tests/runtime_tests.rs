use psyche_rs::{Memory, MemoryStore, Sensation, Will};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;
use uuid::Uuid;

struct MockStore {
    memories: Arc<Mutex<HashMap<Uuid, Memory>>>,
}

impl MockStore {
    fn new() -> Self {
        Self {
            memories: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for MockStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        self.memories
            .lock()
            .await
            .insert(memory.uuid(), memory.clone());
        Ok(())
    }
    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.memories.lock().await.get(&uuid).cloned())
    }
    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn recent_since(&self, _: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<psyche_rs::Impression>> {
        Ok(vec![])
    }
    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }
    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn will_writes_memory_to_store() -> anyhow::Result<()> {
    let store = Arc::new(MockStore::new()) as Arc<dyn MemoryStore>;
    let will = Will::new(store.clone());
    let mem = Memory::Sensation(Sensation {
        uuid: Uuid::new_v4(),
        kind: "test".into(),
        from: "unit".into(),
        payload: json!({"x":1}),
        timestamp: SystemTime::now(),
    });
    will.remember(mem.clone()).await?;
    assert!(store.get_by_uuid(mem.uuid()).await?.is_some());
    Ok(())
}
