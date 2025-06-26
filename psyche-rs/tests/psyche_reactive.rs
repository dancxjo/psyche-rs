use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::LocalSet;
use uuid::Uuid;

use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{
    Psyche,
    memory::{Impression, Memory, MemoryStore, Sensation},
};

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
#[async_trait]
impl MemoryStore for MemStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        self.data.lock().await.insert(memory.uuid(), memory.clone());
        Ok(())
    }
    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.data.lock().await.get(&uuid).cloned())
    }
    async fn recent(&self, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn recent_since(&self, _s: SystemTime) -> anyhow::Result<Vec<Memory>> {
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
impl ChatProvider for DummyLLM {
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
            "jump".to_string()
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

#[tokio::test(flavor = "current_thread")]
async fn sensation_flows_to_intention() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let store = Arc::new(MemStore::new());
            let llm = Arc::new(DummyLLM);
            let psyche = Psyche::new(
                store,
                llm,
                Arc::new(psyche_rs::DummyMouth),
                Arc::new(psyche_rs::DummyMotor::new()),
                "You are Pete".into(),
            );
            let mut rx = psyche.will.receiver.resubscribe();

            for i in 0..3 {
                psyche
                    .send_sensation(Sensation::new_text(format!("hi{}", i), "test"))
                    .await
                    .unwrap();
            }

            let intent = rx.recv().await.unwrap();
            assert_eq!(intent.action.name, "jump");
        })
        .await;
}
