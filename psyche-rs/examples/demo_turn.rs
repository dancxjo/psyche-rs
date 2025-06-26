//! Minimal end-to-end demo showing a single perception \u2192 cognition \u2192
//! action loop.
//!
//! This example feeds a text [`Sensation`] into [`Psyche`] and prints the
//! resulting [`MotorEvent`] stream using [`MotorLog`].

use psyche_rs::*;
use std::pin::Pin;
use std::sync::Arc;
use tokio::task::LocalSet;

use ::llm::ToolCall;
use ::llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use ::llm::error::LLMError;

/// Very small LLM implementation used for the demo.
///
/// When asked for an action suggestion (prompts starting with "List one"), it
/// replies with `say`. All other prompts yield a small XML snippet instructing
/// Pete to greet the user.
struct DemoLLM;

#[async_trait::async_trait]
impl ChatProvider for DemoLLM {
    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: Option<&[::llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, LLMError> {
        Ok(Box::new(SimpleResp(String::new())))
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<Pin<Box<dyn futures_util::Stream<Item = Result<String, LLMError>> + Send>>, LLMError>
    {
        let prompt = messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let reply = if prompt.starts_with("List one") {
            "say".to_string()
        } else {
            "<say pitch=\"gentle\">Hello human</say>".to_string()
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

    fn tool_calls(&self) -> Option<Vec<ToolCall>> {
        None
    }
}

impl std::fmt::Display for SimpleResp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let local = LocalSet::new();
    local
        .run_until(async move {
            let psyche = Psyche::new(
                Arc::new(DummyStore::new()),
                Arc::new(DemoLLM),
                Arc::new(DummyMouth),
                Arc::new(MotorLog),
            );

            psyche
                .send_sensation(Sensation::new_text("Hello Pete, please say hi.", "demo"))
                .await
                .unwrap();

            // Allow some time for the async pipeline to process the input.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        })
        .await;

    Ok(())
}
