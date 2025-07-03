use crate::llm_client::{LLMClient, LLMTokenStream};
use async_trait::async_trait;
use futures::TryStreamExt;
use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, ChatMessageResponseStream, request::ChatMessageRequest},
    models::ModelOptions,
};
use rand::Rng;

/// Build a chat request for the given model and messages.
fn build_request(model: &str, messages: &[ChatMessage]) -> ChatMessageRequest {
    let mut rng = rand::thread_rng();
    let temperature = rng.gen_range(0.5..=1.0);
    tracing::trace!(%temperature, "llm temperature");
    ChatMessageRequest::new(model.to_string(), messages.to_vec())
        .options(ModelOptions::default().temperature(temperature))
}

/// Map an Ollama response stream into an [`LLMTokenStream`].
fn map_stream(stream: ChatMessageResponseStream) -> LLMTokenStream {
    let mapped = stream
        .map_err(|_| Box::<dyn std::error::Error + Send + Sync>::from("ollama stream error"))
        .map_ok(|resp| {
            let tok = resp.message.content;
            tracing::trace!(%tok, "llm token");
            tok
        });
    Box::pin(mapped)
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

    /// Returns the configured model name.
    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl LLMClient for OllamaLLM {
    /// Streams text fragments produced by the model in response to `messages`.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let req = build_request(&self.model, messages);
        let stream = self.client.send_chat_messages_stream(req).await?;
        Ok(map_stream(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use httpmock::prelude::*;
    use reqwest::Client;
    use url::Url;

    #[tokio::test]
    async fn yields_all_tokens() {
        let server = MockServer::start_async().await;
        let body = concat!(
            "{\"model\":\"m\",\"created_at\":\"n\",\"message\":{\"role\":\"assistant\",\"content\":\"he\"},\"done\":false}\n",
            "{\"model\":\"m\",\"created_at\":\"n\",\"message\":{\"role\":\"assistant\",\"content\":\"llo\"},\"done\":true}"
        );
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/chat");
                then.status(200).body(body);
            })
            .await;

        let http = Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let url = Url::parse(&server.base_url()).unwrap();
        let host = format!("{}://{}", url.scheme(), url.host_str().unwrap());
        let port = url.port_or_known_default().unwrap();
        let client = Ollama::new_with_client(host, port, http);
        let llm = OllamaLLM::new(client, "m");
        let msgs = [ChatMessage::user("hi".into())];
        let mut stream = llm.chat_stream(&msgs).await.unwrap();
        let mut collected = String::new();
        while let Some(tok) = stream.next().await {
            collected.push_str(&tok.unwrap());
        }
        assert_eq!(collected, "hello");
    }
}
