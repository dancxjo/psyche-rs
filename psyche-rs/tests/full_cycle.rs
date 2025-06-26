use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::LLMExt;
use psyche_rs::{
    memory::{Completion, Emotion, Impression, IntentionStatus, Memory, MemoryStore, Sensation},
    mouth::Mouth,
    narrator::Narrator,
    voice::Voice,
    wit::Wit,
    wits::{fond::FondDuCoeur, quick::Quick, will::Will},
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

/// Simple in-memory store used by the full cycle test.
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
        Ok(vec![])
    }

    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        let mut items = Vec::new();
        for m in self.data.lock().await.values() {
            if m.timestamp().unwrap_or(SystemTime::UNIX_EPOCH) > since {
                let copy = match m {
                    Memory::Of(boxed) => {
                        if let Some(e) = boxed.downcast_ref::<Emotion>() {
                            Memory::Of(Box::new(e.clone()))
                        } else {
                            Memory::Of(Box::new(()))
                        }
                    }
                    other => other.clone(),
                };
                items.push(copy);
            }
        }
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

    async fn complete_intention(
        &self,
        intention_id: Uuid,
        completion: Completion,
    ) -> anyhow::Result<()> {
        self.save(&Memory::Completion(completion.clone())).await?;
        if let Some(Memory::Intention(i)) = self.data.lock().await.get_mut(&intention_id) {
            i.status = IntentionStatus::Completed;
            i.resolved_at = Some(completion.timestamp);
        }
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

/// Deterministic LLM implementation for the integration test.
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
        messages: &[ChatMessage],
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
        >,
        llm::error::LLMError,
    > {
        let prompt = messages.last().unwrap().content.clone();
        let reply = if prompt.starts_with("List one") {
            "pounce".to_string()
        } else if prompt.starts_with("Summarize the following observations") {
            let count = prompt.lines().count() - 1;
            format!("noticed {} events", count)
        } else {
            messages.last().unwrap().content.clone()
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

/// Mouth implementation that records spoken phrases.
struct DummyMouth {
    log: Arc<Mutex<Vec<String>>>,
}

impl DummyMouth {
    fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Self { log: log.clone() }, log)
    }
}

#[async_trait::async_trait(?Send)]
impl Mouth for DummyMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        self.log.lock().unwrap().push(phrase.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn day_in_the_life_of_pete() {
    let store = Arc::new(DummyMemoryStore::new());
    let llm = Arc::new(DummyLLM);
    let mut quick = Quick::new(store.clone(), llm.clone(), "You are Pete".into());
    let mut will = Will::new(store.clone());
    let mut fond = FondDuCoeur::new(store.clone(), llm.clone(), "You are Pete".into());
    let narrator = Narrator {
        store: store.clone(),
        llm: llm.clone(),
        retriever: Arc::new(psyche_rs::store::embedding_store::NoopRetriever),
    };
    let (mouth, log) = DummyMouth::new();
    let mouth = Arc::new(mouth);
    let mut voice = Voice::new(narrator, mouth, store.clone(), "You are Pete".into());

    for i in 0..3 {
        let s = Sensation {
            uuid: Uuid::new_v4(),
            kind: "text/plain".into(),
            from: "test".into(),
            payload: json!({ "content": format!("something happened {i}") }),
            timestamp: SystemTime::now(),
        };
        quick.observe(s).await;
    }

    let imp = quick
        .distill()
        .await
        .expect("Quick should return an impression");

    let urges = llm
        .suggest_urges("You are Pete", &imp)
        .await
        .expect("LLM returned urges");
    for u in urges {
        will.observe(u).await;
    }

    let intent = will
        .distill()
        .await
        .expect("Will should return an intention");

    let comp = Completion {
        uuid: Uuid::new_v4(),
        intention: intent.uuid,
        outcome: "success".into(),
        transcript: Some("I pounced.".into()),
        timestamp: SystemTime::now(),
    };
    store
        .complete_intention(intent.uuid, comp.clone())
        .await
        .expect("complete ok");

    fond.observe(Memory::Completion(comp)).await;
    let emo_mem = fond.distill().await.expect("Fond should emit emotion");
    if let Memory::Of(boxed) = &emo_mem {
        assert!(boxed.downcast_ref::<Emotion>().is_some());
    } else {
        panic!("expected emotion memory");
    }

    let result = voice.answer_memory_query("today").await;
    assert!(result.is_ok());
    assert!(!log.lock().unwrap().is_empty());
}
