use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use async_stream::stream;
use ollama_rs::{
    Ollama, generation::chat::ChatMessage, generation::chat::request::ChatMessageRequest,
};
use tokio_stream::StreamExt;

/// Stream of LLM tokens.
pub type TokenStream =
    Pin<Box<dyn Stream<Item = Result<String, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

/// Common interface for chat-based LLMs.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Streams tokens in response to chat messages.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>>;
}

/// [`LLMClient`] implementation backed by [`Ollama`].
#[derive(Clone)]
pub struct OllamaLLM {
    client: Ollama,
    model: String,
}

impl OllamaLLM {
    /// Creates a new Ollama-backed client.
    pub fn new(client: Ollama, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }
}

#[async_trait]
impl LLMClient for OllamaLLM {
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let req = ChatMessageRequest::new(self.model.clone(), messages.to_vec());
        let mut stream = self.client.send_chat_messages_stream(req).await?;
        let mapped = stream! {
            while let Some(item) = stream.next().await {
                match item {
                    Ok(resp) => {
                        let tok = resp.message.content;
                        tracing::trace!(%tok, "llm token");
                        yield Ok(tok);
                    }
                    Err(_) => {
                        yield Err(Box::<dyn std::error::Error + Send + Sync>::from("stream error"));
                    }
                }
            }
        };
        Ok(Box::pin(mapped))
    }
}
