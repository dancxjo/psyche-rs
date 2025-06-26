use psyche_rs::action::action;
use psyche_rs::{Completion, Intention, IntentionStatus, Interruption, Memory, MemoryStore, Urge};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use uuid::Uuid;

/// A simple in-memory implementation of [`MemoryStore`] used for tests.
struct MockMemoryStore {
    memories: Arc<Mutex<HashMap<Uuid, Memory>>>,
}

impl MockMemoryStore {
    fn new() -> Self {
        Self {
            memories: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for MockMemoryStore {
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

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let mut values: Vec<_> = self.memories.lock().await.values().cloned().collect();
        values.sort_by_key(|m| std::cmp::Reverse(m.timestamp().unwrap()));
        values.truncate(limit);
        Ok(values)
    }

    async fn of_type(&self, type_name: &str, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let mut values: Vec<_> = self
            .memories
            .lock()
            .await
            .values()
            .cloned()
            .filter(|m| {
                matches!(m, Memory::Sensation(_) if type_name == "Sensation")
                    || matches!(m, Memory::Impression(_) if type_name == "Impression")
                    || matches!(m, Memory::Urge(_) if type_name == "Urge")
                    || matches!(m, Memory::Intention(_) if type_name == "Intention")
                    || matches!(m, Memory::Completion(_) if type_name == "Completion")
                    || matches!(m, Memory::Interruption(_) if type_name == "Interruption")
            })
            .collect();
        values.sort_by_key(|m| std::cmp::Reverse(m.timestamp().unwrap()));
        values.truncate(limit);
        Ok(values)
    }

    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        let mut values: Vec<_> = self
            .memories
            .lock()
            .await
            .values()
            .cloned()
            .filter(|m| m.timestamp().unwrap_or(SystemTime::UNIX_EPOCH) > since)
            .collect();
        values.sort_by_key(|m| m.timestamp().unwrap());
        Ok(values)
    }

    async fn impressions_containing(
        &self,
        keyword: &str,
    ) -> anyhow::Result<Vec<psyche_rs::Impression>> {
        let mut values: Vec<_> = self
            .memories
            .lock()
            .await
            .values()
            .filter_map(|m| match m {
                Memory::Impression(i) if i.how.to_lowercase().contains(&keyword.to_lowercase()) => {
                    Some(i.clone())
                }
                _ => None,
            })
            .collect();
        values.sort_by_key(|i| i.timestamp);
        Ok(values)
    }

    async fn complete_intention(
        &self,
        intention_id: Uuid,
        completion: Completion,
    ) -> anyhow::Result<()> {
        self.save(&Memory::Completion(completion.clone())).await?;
        if let Some(Memory::Intention(intention)) =
            self.memories.lock().await.get_mut(&intention_id)
        {
            intention.status = IntentionStatus::Completed;
            intention.resolved_at = Some(completion.timestamp);
        }
        Ok(())
    }

    async fn interrupt_intention(
        &self,
        intention_id: Uuid,
        interruption: Interruption,
    ) -> anyhow::Result<()> {
        self.save(&Memory::Interruption(interruption.clone()))
            .await?;
        if let Some(Memory::Intention(intention)) =
            self.memories.lock().await.get_mut(&intention_id)
        {
            intention.status = IntentionStatus::Interrupted;
            intention.resolved_at = Some(interruption.timestamp);
        }
        Ok(())
    }
}

fn example_urge(ts: SystemTime) -> Urge {
    Urge {
        uuid: Uuid::new_v4(),
        source: Uuid::new_v4(),
        action: action::with("test", json!({"x": 1})),
        intensity: 0.7,
        timestamp: ts,
    }
}

#[tokio::test]
async fn save_and_get_urge_roundtrip() -> anyhow::Result<()> {
    let store = MockMemoryStore::new();
    let urge = example_urge(SystemTime::now());
    store.save(&Memory::Urge(urge.clone())).await?;

    let retrieved = store.get_by_uuid(urge.uuid).await?.unwrap();
    match retrieved {
        Memory::Urge(u) => {
            assert_eq!(u.uuid, urge.uuid);
            assert_eq!(u.source, urge.source);
            assert_eq!(u.action, urge.action);
            assert_eq!(u.intensity, urge.intensity);
        }
        other => panic!("expected Urge, got {:?}", other),
    }
    Ok(())
}

#[tokio::test]
async fn recent_memories_are_ordered() -> anyhow::Result<()> {
    let store = MockMemoryStore::new();
    for i in 0..5 {
        let ts = SystemTime::now() - Duration::from_secs(i);
        let urge = example_urge(ts);
        store.save(&Memory::Urge(urge)).await?;
    }

    let recent = store.recent(3).await?;
    assert_eq!(recent.len(), 3);
    let t0 = recent[0].timestamp().unwrap();
    let t1 = recent[1].timestamp().unwrap();
    assert!(t0 >= t1);
    Ok(())
}

#[tokio::test]
async fn completing_an_intention_updates_status() -> anyhow::Result<()> {
    let store = MockMemoryStore::new();
    let urge = example_urge(SystemTime::now());
    store.save(&Memory::Urge(urge.clone())).await?;

    let intention = Intention {
        uuid: Uuid::new_v4(),
        urge: urge.uuid,
        action: urge.action.clone(),
        issued_at: SystemTime::now(),
        resolved_at: None,
        status: IntentionStatus::Pending,
    };
    store.save(&Memory::Intention(intention.clone())).await?;

    let completion = Completion {
        uuid: Uuid::new_v4(),
        intention: intention.uuid,
        outcome: "ok".into(),
        transcript: None,
        timestamp: SystemTime::now(),
    };
    store
        .complete_intention(intention.uuid, completion.clone())
        .await?;

    let updated = store.get_by_uuid(intention.uuid).await?.unwrap();
    match updated {
        Memory::Intention(ref i) => {
            assert!(matches!(i.status, IntentionStatus::Completed));
            assert!(i.resolved_at.is_some());
        }
        _ => panic!("expected Intention"),
    }

    let fetched_completion = store.get_by_uuid(completion.uuid).await?.unwrap();
    matches!(fetched_completion, Memory::Completion(_));
    Ok(())
}

#[tokio::test]
async fn interrupting_an_intention_updates_status() -> anyhow::Result<()> {
    let store = MockMemoryStore::new();
    let urge = example_urge(SystemTime::now());
    store.save(&Memory::Urge(urge.clone())).await?;

    let intention = Intention {
        uuid: Uuid::new_v4(),
        urge: urge.uuid,
        action: urge.action.clone(),
        issued_at: SystemTime::now(),
        resolved_at: None,
        status: IntentionStatus::Pending,
    };
    store.save(&Memory::Intention(intention.clone())).await?;

    let interruption = Interruption {
        uuid: Uuid::new_v4(),
        intention: intention.uuid,
        reason: "oops".into(),
        timestamp: SystemTime::now(),
    };
    store
        .interrupt_intention(intention.uuid, interruption.clone())
        .await?;

    let updated = store.get_by_uuid(intention.uuid).await?.unwrap();
    match updated {
        Memory::Intention(ref i) => {
            assert!(matches!(i.status, IntentionStatus::Interrupted));
            assert!(i.resolved_at.is_some());
        }
        _ => panic!("expected Intention"),
    }

    let fetched_interruption = store.get_by_uuid(interruption.uuid).await?.unwrap();
    matches!(fetched_interruption, Memory::Interruption(_));
    Ok(())
}
