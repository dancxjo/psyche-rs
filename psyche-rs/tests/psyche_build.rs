use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use psyche_rs::{
    Psyche,
    countenance::Countenance,
    memory::{Impression, Memory, MemoryStore},
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

use llm::chat::{ChatMessage, ChatProvider, ChatResponse};

struct DummyLLM;

#[async_trait]
impl ChatProvider for DummyLLM {
    async fn chat_with_tools(
        &self,
        _m: &[ChatMessage],
        _t: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
        Ok(Box::new(SimpleResp("".into())))
    }

    async fn chat_stream(
        &self,
        _m: &[ChatMessage],
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
        >,
        llm::error::LLMError,
    > {
        Ok(Box::pin(futures_util::stream::once(async {
            Ok("".into())
        })))
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
