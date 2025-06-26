use async_trait::async_trait;
use std::sync::Arc;
use tokio::task::LocalSet;
use uuid::Uuid;

use psyche_rs::{
    Psyche,
    memory::{Impression, Memory, MemoryStore, Sensation},
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

#[tokio::test(flavor = "current_thread")]
async fn psyche_construction() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let store = Arc::new(DummyStore);
            let llm = Arc::new(DummyLLM);

            let psyche = Psyche::new(
                store,
                llm,
                Arc::new(psyche_rs::DummyMouth),
                Arc::new(psyche_rs::DummyMotor::new()),
            );
            psyche
                .send_sensation(Sensation::new_text("hi", "test"))
                .await
                .unwrap();

            // ensure channels are usable
            let _ = psyche.quick.sender.capacity();
        })
        .await;
}
