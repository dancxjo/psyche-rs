use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{
    Psyche,
    countenance::Countenance,
    memory::{Impression, Memory, MemoryStore, Sensation},
    mouth::Mouth,
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
#[async_trait::async_trait]
impl ChatProvider for SimpleLLM {
    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
        Ok(Box::new(SimpleResp("".into())))
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
        >,
        llm::error::LLMError,
    > {
        let prompt = messages.last().unwrap().content.clone();
        let reply = if prompt.starts_with("List one") {
            "test".to_string()
        } else {
            "summary".to_string()
        };
        Ok(Box::pin(futures_util::stream::once(
            async move { Ok(reply) },
        )))
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
    let (mouth, count) = CountingMouth::new();
    let mouth = Arc::new(mouth);
    struct DummyFace;
    impl Countenance for DummyFace {
        fn reflect(&self, _m: &str) {}
    }

    let (tx, rx) = mpsc::channel(1);
    let (stop_tx, stop_rx) = oneshot::channel();

    let psyche = Psyche::new(
        store.clone(),
        llm,
        mouth.clone(),
        Arc::new(DummyFace),
        rx,
        stop_tx,
        "model".into(),
        "sys".into(),
        128,
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
