use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{
    Emotion,
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

    async fn recent_since(&self, _: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn impressions_containing(&self, _: &str) -> anyhow::Result<Vec<psyche_rs::Impression>> {
        Ok(Vec::new())
    }
}

struct MockLLM;

#[async_trait::async_trait]
impl ChatProvider for MockLLM {
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
            Ok("I feel happy because things worked".into())
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
