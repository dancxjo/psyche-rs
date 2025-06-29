use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use ollama_rs::generation::chat::ChatMessage;

/// Stream of LLM-generated text fragments.
pub type LLMTokenStream =
    Pin<Box<dyn Stream<Item = Result<String, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

/// Common interface for chat-based LLMs.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Streams text fragments in response to chat messages.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>>;
}
