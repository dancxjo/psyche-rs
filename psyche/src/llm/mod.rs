pub mod chat;
pub mod embed;
pub mod mock_chat;
pub mod mock_embed;
pub mod prompt;

use async_trait::async_trait;
use tokio_stream::Stream;

/// Supported capabilities for a language model backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmCapability {
    /// Conversational text generation.
    Chat,
    /// Text embedding vectors.
    Embedding,
    /// Image generation or understanding.
    Image,
    /// Tool use / function calling.
    ToolUse,
}

/// Description of a specific language model profile.
#[derive(Debug, Clone)]
pub struct LlmProfile {
    /// Provider name such as "ollama" or "openai".
    pub provider: String,
    /// Model identifier like "llama3" or "gpt-4o".
    pub model: String,
    /// Capabilities supported by this model.
    pub capabilities: Vec<LlmCapability>,
}

/// Interface for models capable of chatting.
#[async_trait(?Send)]
pub trait CanChat {
    /// Streams the model\'s chat completion as individual tokens.
    async fn chat_stream(
        &self,
        profile: &LlmProfile,
        system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>>;
}

/// Interface for models capable of producing embeddings.
#[async_trait(?Send)]
pub trait CanEmbed {
    /// Returns an embedding vector for the supplied text.
    async fn embed(&self, profile: &LlmProfile, text: &str) -> anyhow::Result<Vec<f32>>;
}

/// Registry holding implementations for each capability.
pub struct LlmRegistry {
    /// Chat implementation.
    pub chat: Box<dyn CanChat>,
    /// Embedding implementation.
    pub embed: Box<dyn CanEmbed>,
}
