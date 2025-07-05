use crate::llm::types::{Token, TokenStream};
use crate::llm_client::LLMClient;
use async_trait::async_trait;
use futures::StreamExt;
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

/// Map an Ollama response stream into a [`TokenStream`].
fn map_stream(stream: ChatMessageResponseStream) -> TokenStream {
    let mapped = stream.filter_map(|res| async {
        match res {
            Ok(resp) => {
                let tok = resp.message.content;
                tracing::trace!(%tok, "llm token");
                Some(Token { text: tok })
            }
            Err(e) => {
                tracing::error!(?e, "ollama stream error");
                None
            }
        }
    });
    Box::pin(mapped)
}

/// [`LLMClient`] implementation backed by [`Ollama`].
#[derive(Clone)]
pub struct OllamaLLM {
    client: Ollama,
    model: String,
    embedding_model: String,
}

impl OllamaLLM {
    /// Creates a new Ollama-backed client.
    pub fn new(client: Ollama, model: impl Into<String>) -> Self {
        Self::with_embedding_model(client, model, "nomic-embed-text")
    }

    /// Creates a new client with a custom embedding model.
    pub fn with_embedding_model(
        client: Ollama,
        model: impl Into<String>,
        embedding_model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            model: model.into(),
            embedding_model: embedding_model.into(),
        }
    }

    /// Returns the configured model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the embedding model name.
    pub fn embedding_model(&self) -> &str {
        &self.embedding_model
    }
}

#[async_trait]
impl LLMClient for OllamaLLM {
    /// Streams text fragments produced by the model in response to `messages`.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let req = build_request(&self.model, messages);
        let stream = self.client.send_chat_messages_stream(req).await?;
        Ok(map_stream(stream))
    }

    async fn embed(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        use ollama_rs::generation::embeddings::request::GenerateEmbeddingsRequest;
        let res = self
            .client
            .generate_embeddings(GenerateEmbeddingsRequest::new(
                self.embedding_model.clone(),
                text.into(),
            ))
            .await?;
        Ok(res.embeddings.into_iter().next().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use httpmock::prelude::*;
    use reqwest::Client;
    use serde_json::json;
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
            collected.push_str(&tok.text);
        }
        assert_eq!(collected, "hello");
    }

    #[tokio::test]
    async fn uses_embedding_model() {
        let server = MockServer::start_async().await;
        let _m = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/embed");
                then.status(200)
                    .json_body(json!({"embeddings": [[0.1, 0.2]]}));
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
        let llm = OllamaLLM::with_embedding_model(client, "m", "e");
        let vec = llm.embed("hi").await.unwrap();
        assert!((vec[0] - 0.1).abs() < f32::EPSILON);
    }
}
