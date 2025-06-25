use psyche_rs::{
    llm::LLMClient,
    memory::{Impression, Memory, MemoryStore, Sensation},
    mouth::Mouth,
    narrator::Narrator,
    voice::Voice,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
        self.data.lock().await.insert(memory.uuid(), memory.clone());
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.data.lock().await.get(&uuid).cloned())
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        let mut items: Vec<_> = self
            .data
            .lock()
            .await
            .values()
            .cloned()
            .filter(|m| m.timestamp().unwrap_or(SystemTime::UNIX_EPOCH) > since)
            .collect();
        items.sort_by_key(|m| m.timestamp().unwrap());
        Ok(items)
    }

    async fn impressions_containing(&self, keyword: &str) -> anyhow::Result<Vec<Impression>> {
        let mut items: Vec<_> = self
            .data
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
        items.sort_by_key(|i| i.timestamp);
        Ok(items)
    }

    async fn complete_intention(&self, _id: Uuid, _c: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(
        &self,
        _id: Uuid,
        _i: psyche_rs::Interruption,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

struct EchoLLM;

#[async_trait::async_trait]
impl LLMClient for EchoLLM {
    async fn summarize(&self, _input: &[Sensation]) -> anyhow::Result<String> {
        Ok(String::new())
    }

    async fn summarize_impressions(&self, items: &[Impression]) -> anyhow::Result<String> {
        Ok(items
            .iter()
            .map(|i| i.how.clone())
            .collect::<Vec<_>>()
            .join(" "))
    }

    async fn suggest_urges(&self, _imp: &Impression) -> anyhow::Result<Vec<psyche_rs::Urge>> {
        Ok(vec![])
    }

    async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
        Ok(String::new())
    }
}

struct LoggingMouth {
    log: Arc<Mutex<Vec<String>>>,
}

impl LoggingMouth {
    fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Self { log: log.clone() }, log)
    }
}

#[async_trait::async_trait(?Send)]
impl Mouth for LoggingMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        self.log.lock().unwrap().push(phrase.to_string());
        Ok(())
    }
}

fn make_impression(text: &str, ts: SystemTime) -> Impression {
    Impression {
        uuid: Uuid::new_v4(),
        how: text.into(),
        topic: "test".into(),
        composed_of: vec![],
        timestamp: ts,
    }
}

#[tokio::test]
async fn test_narrate_response() -> anyhow::Result<()> {
    let store = Arc::new(MockStore::new());
    let llm = Arc::new(EchoLLM);
    let narrator = Narrator {
        store: store.clone(),
        llm,
    };
    let (mouth, log) = LoggingMouth::new();
    let mouth = Arc::new(mouth);

    let mut voice = Voice::new(narrator, mouth.clone(), store.clone());

    let now = SystemTime::now();
    for text in ["saw a bird", "ate lunch", "took a nap"] {
        store
            .save(&Memory::Impression(make_impression(text, now)))
            .await?;
    }

    voice.answer_memory_query("today").await?;

    assert_eq!(log.lock().unwrap().len(), 1);
    let spoken = &log.lock().unwrap()[0];
    assert!(spoken.contains("saw a bird"));

    let count = store
        .data
        .lock()
        .await
        .values()
        .filter(|m| matches!(m, Memory::Sensation(_)))
        .count();
    assert_eq!(count, 1);

    Ok(())
}
