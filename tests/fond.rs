use psyche_rs::{
    Emotion,
    llm::LLMClient,
    memory::{Completion, Interruption, Memory, MemoryStore},
    wit::Wit,
    wits::fond::FondDuCoeur,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

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
        let stored = match memory {
            Memory::Of(boxed) => {
                if let Some(e) = boxed.downcast_ref::<Emotion>() {
                    Memory::Of(Box::new(e.clone()))
                } else {
                    Memory::Of(Box::new(()))
                }
            }
            other => other.clone(),
        };
        self.data.lock().await.insert(memory.uuid(), stored);
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        let guard = self.data.lock().await;
        let mem = guard.get(&uuid).map(|m| match m {
            Memory::Of(boxed) => {
                if let Some(e) = boxed.downcast_ref::<Emotion>() {
                    Memory::Of(Box::new(e.clone()))
                } else {
                    Memory::Of(Box::new(()))
                }
            }
            other => other.clone(),
        });
        Ok(mem)
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }
    async fn complete_intention(&self, _: Uuid, _: Completion) -> anyhow::Result<()> {
        Ok(())
    }
    async fn interrupt_intention(&self, _: Uuid, _: Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

struct MockLLM;

#[async_trait::async_trait]
impl LLMClient for MockLLM {
    async fn summarize(&self, _input: &[psyche_rs::Sensation]) -> anyhow::Result<String> {
        Ok(String::new())
    }
    async fn suggest_urges(
        &self,
        _imp: &psyche_rs::Impression,
    ) -> anyhow::Result<Vec<psyche_rs::Urge>> {
        Ok(vec![])
    }
    async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
        Ok("I feel happy because things worked".into())
    }
}

fn sample_completion() -> Memory {
    Memory::Completion(Completion {
        uuid: Uuid::new_v4(),
        intention: Uuid::new_v4(),
        outcome: "ok".into(),
        transcript: None,
        timestamp: SystemTime::now(),
    })
}

#[tokio::test]
async fn fond_du_coeur_creates_emotion() {
    let store = Arc::new(MockStore::new());
    let llm = Arc::new(MockLLM);
    let mut fond = FondDuCoeur::new(store.clone(), llm);

    let event = sample_completion();
    fond.observe(event.clone()).await;

    let mem = fond.distill().await.expect("should output emotion");

    let saved = store.get_by_uuid(mem.uuid()).await.unwrap();
    assert!(saved.is_some());

    if let Memory::Of(boxed) = mem {
        let emo = boxed.downcast_ref::<Emotion>().expect("should be emotion");
        assert_eq!(emo.subject, event.uuid());
        assert_eq!(emo.mood, "happy");
    } else {
        panic!("expected custom memory");
    }
}
