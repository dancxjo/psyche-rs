use crate::memory::{Impression, Memory, Sensation, Urge};
use anyhow::Result;
use async_trait::async_trait;
use llm::LLMProvider;
use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use std::sync::Arc;
use uuid::Uuid;

/// Abstract interface for language model interactions used by cognitive wits.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Summarize a slice of [`Sensation`]s into a natural language description.
    async fn summarize(&self, input: &[Sensation]) -> Result<String>;

    /// Summarize a sequence of [`Impression`]s into a higher-level summary.
    async fn summarize_impressions(&self, items: &[Impression]) -> Result<String>;

    /// Suggest potential [`Urge`]s based on the given [`Impression`].
    async fn suggest_urges(&self, impression: &Impression) -> Result<Vec<Urge>>;

    /// Evaluate an emotional reaction to a completed or interrupted intention.
    async fn evaluate_emotion(&self, event: &Memory) -> Result<String>;
}

/// Trivial implementation used for testing.
pub struct DummyLLM;

/// Adapter type wrapping a [`ChatProvider`] for use where an [`LLMClient`] is
/// expected.
#[derive(Clone)]
pub struct ChatLLM(pub Arc<dyn LLMProvider>);

#[async_trait]
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

#[async_trait]
impl LLMClient for DummyLLM {
    async fn summarize(&self, input: &[Sensation]) -> Result<String> {
        Ok(format!("I'm seeing {} sensations.", input.len()))
    }

    async fn suggest_urges(&self, _impression: &Impression) -> Result<Vec<Urge>> {
        Ok(vec![])
    }

    async fn evaluate_emotion(&self, _event: &Memory) -> Result<String> {
        Ok("I feel indifferent.".to_string())
    }

    async fn summarize_impressions(&self, items: &[Impression]) -> Result<String> {
        Ok(format!("I'm recalling {} impressions.", items.len()))
    }
}

#[async_trait]
impl<T> LLMClient for T
where
    T: ChatProvider + Send + Sync + ?Sized,
{
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
        let req = vec![ChatMessage::user().content(prompt).build()];
        let resp = self.chat(&req).await?;
        Ok(resp.text().unwrap_or_default())
    }

    async fn summarize_impressions(&self, items: &[Impression]) -> Result<String> {
        let mut prompt = String::from("Summarize the following impressions:\n");
        for i in items {
            prompt.push_str("- ");
            prompt.push_str(&i.how);
            prompt.push('\n');
        }
        let req = vec![ChatMessage::user().content(prompt).build()];
        let resp = self.chat(&req).await?;
        Ok(resp.text().unwrap_or_default())
    }

    async fn suggest_urges(&self, impression: &Impression) -> Result<Vec<Urge>> {
        let prompt = format!(
            "List one suggested motor action for: {}. Respond with just the action name.",
            impression.how
        );
        let req = vec![ChatMessage::user().content(prompt).build()];
        let resp = self.chat(&req).await?;
        let text = resp.text().unwrap_or_default();
        if text.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![Urge {
            uuid: Uuid::new_v4(),
            source: impression.uuid,
            motor_name: text,
            parameters: serde_json::json!({}),
            intensity: 1.0,
            timestamp: impression.timestamp,
        }])
    }

    async fn evaluate_emotion(&self, event: &Memory) -> Result<String> {
        let prompt = format!("How should Pete feel about this event? {:?}", event);
        let req = vec![ChatMessage::user().content(prompt).build()];
        let resp = self.chat(&req).await?;
        Ok(resp.text().unwrap_or_default())
    }
}
