use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::wit::{Wit, WitHandle};
use psyche_rs::{
    memory::{Impression, Memory, MemoryStore, Sensation},
    wits::quick::Quick,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc};
use uuid::Uuid;

struct DummyStore {
    data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
}
impl DummyStore {
    fn new() -> Self {
        Self {
            data: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}
#[async_trait::async_trait]
impl MemoryStore for DummyStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        self.data.lock().await.insert(memory.uuid(), memory.clone());
        Ok(())
    }
    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.data.lock().await.get(&uuid).cloned())
    }
    async fn recent(&self, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }
    async fn recent_since(&self, _: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }
    async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<Impression>> {
        Ok(Vec::new())
    }
    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }
    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

struct DummyLLM;
#[async_trait::async_trait]
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
            Ok("summary".into())
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

fn sample() -> Sensation {
    Sensation {
        uuid: Uuid::new_v4(),
        kind: "t".into(),
        from: "test".into(),
        payload: json!({"x":1}),
        timestamp: SystemTime::now(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn quick_emits_via_channels() {
    let store = Arc::new(DummyStore::new());
    let llm = Arc::new(DummyLLM);
    let quick = Quick::new(store, llm, "You are Pete".into());

    let (tx_in, rx_in) = mpsc::channel(4);
    let (tx_out, _) = broadcast::channel(4);
    let mut handle = WitHandle {
        sender: tx_in,
        receiver: tx_out.subscribe(),
    };
    let local = tokio::task::LocalSet::new();
    local.spawn_local(quick.run(rx_in, tx_out));

    local
        .run_until(async move {
            handle.sender.send(sample()).await.unwrap();
            handle.sender.send(sample()).await.unwrap();
            let imp = handle.receiver.recv().await.expect("impression");
            assert_eq!(imp.topic, "instant");
        })
        .await;
}
