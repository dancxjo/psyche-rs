use crate::memory::{Impression, Memory, Sensation, Urge};
use anyhow::Result;
use futures_util::StreamExt;
use llm::LLMProvider;
use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use serde_json;
use std::sync::Arc;
use uuid::Uuid;

/// Adapter type wrapping an [`LLMProvider`] so it can be used as a
/// [`ChatProvider`].
#[derive(Clone)]
pub struct ChatLLM(pub Arc<dyn LLMProvider>);

#[async_trait::async_trait]
impl ChatProvider for ChatLLM {
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
        self.0.chat_with_tools(messages, tools).await
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
        self.0.chat_stream(messages).await
    }
}

/// Convenience extension providing higher level operations on top of
/// [`ChatProvider`]. All methods consume model output via streaming to
/// avoid unnecessary latency.
#[async_trait::async_trait(?Send)]
pub trait LLMExt: ChatProvider + Send + Sync {
    /// Summarize a slice of [`Sensation`]s.
    async fn summarize(&self, input: &[Sensation]) -> Result<String> {
        let mut prompt = String::from("Summarize the following observations in one sentence:\n");
        for s in input {
            let text = s
                .payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            prompt.push_str("- ");
            prompt.push_str(text);
            prompt.push('\n');
        }
        collect_stream(
            self.chat_stream(&[ChatMessage::user().content(prompt).build()])
                .await?,
        )
        .await
    }

    /// Summarize several [`Impression`]s into a single statement.
    async fn summarize_impressions(&self, items: &[Impression]) -> Result<String> {
        let mut prompt = String::from("Summarize the following impressions:\n");
        for i in items {
            prompt.push_str("- ");
            prompt.push_str(&i.how);
            prompt.push('\n');
        }
        collect_stream(
            self.chat_stream(&[ChatMessage::user().content(prompt).build()])
                .await?,
        )
        .await
    }

    /// Suggest a set of [`Urge`]s based on the provided [`Impression`].
    async fn suggest_urges(
        &self,
        system_prompt: &str,
        impression: &Impression,
    ) -> Result<Vec<Urge>> {
        let prompt = format!(
            "List one suggested motor action for: {}. Respond with just the action name.",
            impression.how
        );
        let text = collect_stream(
            self.chat_stream(&[
                ChatMessage::user().content(system_prompt).build(),
                ChatMessage::user().content(prompt).build(),
            ])
            .await?,
        )
        .await?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![Urge {
            uuid: Uuid::new_v4(),
            source: impression.uuid,
            action: crate::action::Action::new(text.trim(), serde_json::json!({})),
            intensity: 1.0,
            timestamp: impression.timestamp,
        }])
    }

    /// Evaluate an emotional response to a given [`Memory`].
    async fn evaluate_emotion(&self, system_prompt: &str, event: &Memory) -> Result<String> {
        let prompt = format!("How should Pete feel about this event? {:?}", event);
        collect_stream(
            self.chat_stream(&[
                ChatMessage::user().content(system_prompt).build(),
                ChatMessage::user().content(prompt).build(),
            ])
            .await?,
        )
        .await
    }
}

#[async_trait::async_trait(?Send)]
impl<T> LLMExt for T where T: ChatProvider + Send + Sync + ?Sized {}

async fn collect_stream(
    mut stream: impl futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Unpin,
) -> Result<String> {
    let mut out = String::new();
    while let Some(part) = stream.next().await {
        out.push_str(&part?);
    }
    Ok(out)
}

/// Basic LLM used for examples which simply echoes the last user message.
#[derive(Clone)]
pub struct DummyLLM;

#[async_trait::async_trait]
impl ChatProvider for DummyLLM {
    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: Option<&[llm::chat::Tool]>,
    ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
        Ok(Box::new(SimpleLLMResp(String::new())))
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
        let reply = messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Ok(Box::pin(futures_util::stream::once(
            async move { Ok(reply) },
        )))
    }
}

#[derive(Debug)]
struct SimpleLLMResp(String);

impl ChatResponse for SimpleLLMResp {
    fn text(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
        None
    }
}

impl std::fmt::Display for SimpleLLMResp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
