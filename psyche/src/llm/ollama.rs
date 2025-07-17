use super::{CanChat, LlmProfile};
use async_stream::stream;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio_stream::Stream;
use tracing::{debug, trace};

/// Chat client that calls an Ollama instance via HTTP.
#[derive(Clone, Debug)]
pub struct OllamaChat {
    /// Base URL for the Ollama server, e.g. `http://localhost:11434`.
    pub base_url: String,
    /// Model name such as `mistral` or `llama3`.
    pub model: String,
}

#[derive(Deserialize)]
struct Chunk {
    message: Option<Message>,
    #[allow(dead_code)]
    done: Option<bool>,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[async_trait(?Send)]
impl CanChat for OllamaChat {
    async fn chat_stream(
        &self,
        _profile: &LlmProfile,
        system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "stream": true
        });
        trace!(target = "llm", %url, body = %body, "Ollama prompt");
        let resp = reqwest::Client::new().post(url).json(&body).send().await?;
        let mut stream = resp.bytes_stream();
        let out = stream! {
            let mut full = String::new();
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        debug!(target = "llm", error = %e, "stream error");
                        break;
                    }
                };
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    if line.trim().is_empty() { continue; }
                    if let Ok(c) = serde_json::from_str::<Chunk>(line) {
                        if let Some(msg) = c.message {
                            trace!(target = "llm", token = %msg.content, "stream token");
                            full.push_str(&msg.content);
                            yield msg.content;
                        }
                    }
                }
            }
            debug!(target = "llm", response = %full, "Ollama full response");
        };
        Ok(Box::new(Box::pin(out)) as Box<dyn Stream<Item = String> + Unpin>)
    }
}
