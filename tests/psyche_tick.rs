use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use psyche_rs::{
    Psyche,
    llm::LLMClient,
    memory::{Impression, Memory, MemoryStore, Sensation, Urge},
    motor::DummyMotor,
    mouth::Mouth,
    narrator::Narrator,
    voice::Voice,
    wits::{fond::FondDuCoeur, quick::Quick, will::Will},
};
use tokio::sync::{mpsc, oneshot};

struct CountingMouth {
    count: Arc<Mutex<usize>>,
}

impl CountingMouth {
    fn new() -> (Self, Arc<Mutex<usize>>) {
        let count = Arc::new(Mutex::new(0));
        (
            Self {
                count: count.clone(),
            },
            count,
        )
    }
}

#[async_trait(?Send)]
impl Mouth for CountingMouth {
    async fn say(&self, _phrase: &str) -> anyhow::Result<()> {
        *self.count.lock().unwrap() += 1;
        Ok(())
    }
}

struct SimpleLLM;

#[async_trait::async_trait]
impl LLMClient for SimpleLLM {
    async fn summarize(&self, _input: &[Sensation]) -> anyhow::Result<String> {
        Ok("summary".into())
    }

    async fn summarize_impressions(&self, _items: &[Impression]) -> anyhow::Result<String> {
        Ok("story".into())
    }

    async fn suggest_urges(&self, _impression: &Impression) -> anyhow::Result<Vec<Urge>> {
        Ok(vec![Urge {
            uuid: Uuid::new_v4(),
            source: Uuid::new_v4(),
            motor_name: "test".into(),
            parameters: serde_json::json!({}),
            intensity: 1.0,
            timestamp: SystemTime::now(),
        }])
    }

    async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
        Ok("fine".into())
    }
}

struct MemStore {
    data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            data: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for MemStore {
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

    async fn recent_since(&self, _since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn impressions_containing(&self, _keyword: &str) -> anyhow::Result<Vec<Impression>> {
        Ok(vec![])
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

#[tokio::test]
async fn tick_drives_voice_turn() {
    let store = Arc::new(MemStore::new());
    let llm = Arc::new(SimpleLLM);
    let motor = Arc::new(DummyMotor);

    let quick = Quick::new(store.clone(), llm.clone());
    let will = Will::new(store.clone(), motor);
    let fond = FondDuCoeur::new(store.clone(), llm.clone());
    let narrator = Narrator {
        store: store.clone(),
        llm: llm.clone(),
    };
    let (mouth, count) = CountingMouth::new();
    let voice = Voice::new(narrator.clone(), Arc::new(mouth), store.clone());

    let (tx, rx) = mpsc::channel(1);
    let (stop_tx, stop_rx) = oneshot::channel();

    let psyche = Psyche::new(
        quick,
        will,
        fond,
        voice,
        narrator,
        store.clone(),
        llm,
        rx,
        stop_tx,
    );
    let voice_handle = psyche.voice.clone();

    let s = Sensation {
        uuid: Uuid::new_v4(),
        kind: "text".into(),
        from: "tester".into(),
        payload: serde_json::json!({ "a": 1 }),
        timestamp: SystemTime::now(),
    };

    tx.send(s).await.unwrap();
    drop(tx);
    psyche.tick().await;
    let _ = stop_rx.await;

    // The voice should have spoken once
    assert_eq!(*count.lock().unwrap(), 1);

    // ensure we can still access voice
    let _ = voice_handle.lock().await.current_mood.clone();
}
