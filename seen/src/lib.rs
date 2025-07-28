use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info};

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use chrono::Utc;
use futures_util::StreamExt;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::images::Image;
use ollama_rs::Ollama;
use psyche::llm::{CanChat, LlmCapability, LlmProfile};
use psyche::models::MemoryEntry;
use psyche::wit::{Wit, WitConfig};
use tokio_stream::Stream;

const PROMPT: &str = "This is what you are currently seeing. It is from your perspective, so whomever you see isn't you, unless you're looking at a mirror or something. Narrate to yourself what you are seeing in one and only one sentence.";

#[derive(Clone)]
struct OllamaImageChat {
    base_url: String,
    model: String,
    images: Vec<Image>,
}

impl OllamaImageChat {
    fn new(base_url: String, model: String, images: Vec<Image>) -> Self {
        Self {
            base_url,
            model,
            images,
        }
    }
}

#[async_trait(?Send)]
impl CanChat for OllamaImageChat {
    async fn chat_stream(
        &self,
        _profile: &LlmProfile,
        system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        let ollama = Ollama::try_new(&self.base_url)?;
        let req = GenerationRequest::new(self.model.clone(), user.to_string())
            .system(system.to_string())
            .images(self.images.clone());
        let mut stream = ollama.generate_stream(req).await?;
        let out = async_stream::stream! {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(resps) => for r in resps { yield r.response },
                    Err(_) => break,
                }
            }
        };
        Ok(Box::new(Box::pin(out)) as Box<dyn Stream<Item = String> + Unpin>)
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    base_url: String,
    model: String,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    if buf.is_empty() {
        return Ok(());
    }
    let b64 = general_purpose::STANDARD.encode(&buf);
    let images = vec![Image::from_base64(b64)];
    let llm = OllamaImageChat::new(base_url.clone(), model.clone(), images);
    let cfg = WitConfig {
        name: "seen".into(),
        input_kind: "sensation/image".into(),
        output_kind: "instant".into(),
        prompt_template: PROMPT.into(),
        post_process: None,
    };
    let profile = LlmProfile {
        provider: "ollama".into(),
        model: model.clone(),
        capabilities: vec![LlmCapability::Chat, LlmCapability::Image],
    };
    let mut wit = Wit {
        config: cfg,
        llm: Box::new(llm),
        profile,
    };
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4(),
        kind: "sensation/image".into(),
        when: Utc::now(),
        what: serde_json::Value::Null,
        how: String::new(),
    };
    let out = wit.distill(vec![entry]).await?;
    let text = out.first().map(|e| e.how.clone()).unwrap_or_default();
    stream.write_all(text.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Run the seen daemon.
pub async fn run(socket: PathBuf, base_url: String, model: String) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "seen listening");
    loop {
        let (stream, _) = listener.accept().await?;
        if let Err(e) = handle_connection(stream, base_url.clone(), model.clone()).await {
            error!(?e, "connection failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use tempfile::tempdir;
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn image_chat_streams_text() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"llava\",\"created_at\":\"now\",\"response\":\"desc\",\"done\":true}\n";
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/generate")
                    .body_contains("\"images\"");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(body);
            })
            .await;
        let chat = OllamaImageChat::new(
            server.base_url(),
            "llava".into(),
            vec![Image::from_base64("abcd")],
        );
        let profile = LlmProfile {
            provider: "ollama".into(),
            model: "llava".into(),
            capabilities: vec![LlmCapability::Chat, LlmCapability::Image],
        };
        let mut stream = chat.chat_stream(&profile, "", "hi").await.unwrap();
        let mut out = String::new();
        while let Some(tok) = stream.next().await {
            out.push_str(&tok);
        }
        assert_eq!(out, "desc");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn run_processes_image() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"llava\",\"created_at\":\"now\",\"response\":\"a cat\",\"done\":true}\n";
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/generate");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(body);
            })
            .await;
        let dir = tempdir().unwrap();
        let sock = dir.path().join("eye.sock");
        let url = server.base_url();
        let local = tokio::task::LocalSet::new();
        let run_fut = local.spawn_local(run(sock.clone(), url, "llava".into()));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut client = UnixStream::connect(&sock).await.unwrap();
                client.write_all(b"PNGdata").await.unwrap();
                client.shutdown().await.unwrap();
                let mut buf = String::new();
                client.read_to_string(&mut buf).await.unwrap();
                assert_eq!(buf, "a cat");
            })
            .await;
        run_fut.abort();
    }
}
