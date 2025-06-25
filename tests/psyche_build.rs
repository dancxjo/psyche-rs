use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use psyche_rs::{
    Psyche,
    countenance::Countenance,
    llm::LLMClient,
    memory::{Impression, Memory, MemoryStore, Sensation, Urge},
    mouth::Mouth,
};

struct DummyStore;

#[async_trait::async_trait]
impl MemoryStore for DummyStore {
    async fn save(&self, _memory: &Memory) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_by_uuid(&self, _uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(None)
    }
    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn recent_since(&self, _s: std::time::SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn impressions_containing(&self, _k: &str) -> anyhow::Result<Vec<Impression>> {
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

struct DummyLLM;

#[async_trait]
impl LLMClient for DummyLLM {
    async fn summarize(&self, _input: &[Sensation]) -> anyhow::Result<String> {
        Ok(String::new())
    }
    async fn summarize_impressions(&self, _items: &[Impression]) -> anyhow::Result<String> {
        Ok(String::new())
    }
    async fn suggest_urges(&self, _imp: &Impression) -> anyhow::Result<Vec<Urge>> {
        Ok(vec![])
    }
    async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
        Ok(String::new())
    }
}

struct SilentMouth;
#[async_trait(?Send)]
impl Mouth for SilentMouth {
    async fn say(&self, _phrase: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NullFace;
impl Countenance for NullFace {
    fn reflect(&self, _mood: &str) {}
}

#[tokio::test]
async fn psyche_construction() {
    let store = Arc::new(DummyStore);
    let llm = Arc::new(DummyLLM);
    let mouth = Arc::new(SilentMouth);
    let face = Arc::new(NullFace);
    let (_tx, rx) = mpsc::channel(1);
    let (stop_tx, _stop_rx) = oneshot::channel();

    let psyche = Psyche::new(
        store,
        llm,
        mouth,
        face,
        rx,
        stop_tx,
        "dummy".into(),
        "system".into(),
        10,
    );

    let _ = psyche.quick.lock().await;
    let _ = psyche.will.lock().await;
    let _ = psyche.fond.lock().await;
    let _ = psyche.voice.lock().await;
    let _ = psyche.narrator.lock().await;
}
