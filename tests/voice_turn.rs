use async_trait::async_trait;
use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::{conversation::Conversation, mouth::Mouth, voice::Voice};
use std::sync::{Arc, Mutex};

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
struct SimpleResponse(String);

impl ChatResponse for SimpleResponse {
    fn text(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
        None
    }
}

impl std::fmt::Display for SimpleResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

struct EchoLLM;

#[async_trait]
impl ChatProvider for EchoLLM {
    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
        Ok(Box::new(SimpleResponse("Hello!".into())))
    }
}

#[tokio::test]
async fn take_turn_speaks_reply() -> anyhow::Result<()> {
    let mut voice = Voice::default();
    voice.conversation = Conversation::new("sys".into(), 10);
    voice.llm = Arc::new(EchoLLM);
    let (mouth, log) = LoggingMouth::new();
    voice.mouth = Arc::new(mouth);

    voice
        .conversation
        .hear(psyche_rs::conversation::Role::Interlocutor, "Hi");
    let reply = voice.take_turn().await?;

    assert_eq!(reply, "Hello!");
    assert_eq!(log.lock().unwrap()[0], "Hello!");
    Ok(())
}
