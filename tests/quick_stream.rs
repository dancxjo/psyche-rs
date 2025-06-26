use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{
    memory::{Memory, MemoryStore, Sensation},
    wit::Wit,
    wits::quick::Quick,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

/// Dummy in-memory store used for streaming tests.
struct DummyMemoryStore {
    data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
}

impl DummyMemoryStore {
    fn new() -> Self {
        Self {
            data: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for DummyMemoryStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        self.data.lock().await.insert(memory.uuid(), memory.clone());
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.data.lock().await.get(&uuid).cloned())
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn recent_since(&self, _: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<psyche_rs::Impression>> {
        Ok(Vec::new())
    }

    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

fn make_fake_sensation(n: usize) -> Sensation {
    Sensation {
        uuid: Uuid::new_v4(),
        kind: "text/plain".into(),
        from: "test".into(),
        payload: json!({ "content": format!("event_{}", n) }),
        timestamp: SystemTime::now(),
    }
}

#[tokio::test]
async fn quick_summarizes_sensations_stream() {
    let store = Arc::new(DummyMemoryStore::new());
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
    let mut quick = Quick::new(store.clone(), Arc::new(DummyLLM));

    for i in 0..15 {
        quick.observe(make_fake_sensation(i)).await;
    }

    let imp = quick.distill().await.expect("Quick failed to summarize");
    println!("Impression: {:?}", imp);

    let saved = store.get_by_uuid(imp.uuid).await.unwrap();
    assert!(matches!(saved, Some(Memory::Impression(_))));
}
