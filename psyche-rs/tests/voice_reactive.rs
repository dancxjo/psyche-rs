use async_trait::async_trait;
use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{DummyStore, Psyche, memory::Sensation, mouth::Mouth};
use std::sync::{Arc, Mutex};
use tokio::task::LocalSet;

struct LoggingMouth {
    log: Arc<Mutex<Vec<String>>>,
}

impl LoggingMouth {
    fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Self { log: log.clone() }, log)
    }
}

#[async_trait(?Send)]
impl Mouth for LoggingMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        self.log.lock().unwrap().push(phrase.to_string());
        Ok(())
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

struct TestLLM;

#[async_trait]
impl ChatProvider for TestLLM {
    async fn chat_with_tools(
        &self,
        _msgs: &[ChatMessage],
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
            "speak".to_string()
        } else if prompt.starts_with("Summarize") {
            "summary".to_string()
        } else {
            "hello".to_string()
        };
        Ok(Box::pin(futures_util::stream::once(
            async move { Ok(reply) },
        )))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn voice_speaks_after_intention() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let (mouth, log) = LoggingMouth::new();
            let psyche = Psyche::new(
                Arc::new(DummyStore::new()),
                Arc::new(TestLLM),
                Arc::new(mouth),
                Arc::new(psyche_rs::DummyMotor::new()),
            );
            let mut intents = psyche.will.receiver.resubscribe();

            for i in 0..3 {
                psyche
                    .send_sensation(Sensation::new_text(format!("hi{}", i), "test"))
                    .await
                    .unwrap();
            }

            let _ = intents.recv().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            assert_eq!(log.lock().unwrap().len(), 1);
            assert_eq!(log.lock().unwrap()[0], "hello");
        })
        .await;
}
