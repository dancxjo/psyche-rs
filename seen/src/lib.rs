use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use base64::{engine::general_purpose, Engine};

use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::completion::GenerationResponseStream;
use ollama_rs::generation::images::Image;
use ollama_rs::Ollama;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, Mutex, Notify};
use tokio_stream::StreamExt;
use tracing::{debug, error, info, trace};

const PROMPT: &str = "This is what you are currently seeing. It is from your perspective, so whomever you see isn't you, unless you're looking at a mirror or something. Narrate to yourself what you are seeing in one and only one sentence.";

#[derive(Clone)]
struct ImageQueue {
    inner: Arc<Mutex<VecDeque<Vec<u8>>>>,
    notify: Arc<Notify>,
}

impl ImageQueue {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    async fn push(&self, img: Vec<u8>) {
        let mut q = self.inner.lock().await;
        q.push_back(img);
        self.notify.notify_one();
    }

    async fn pop(&self) -> Option<Vec<u8>> {
        self.inner.lock().await.pop_front()
    }

    async fn wait(&self) {
        self.notify.notified().await;
    }
}

/// Return the byte index just after the JPEG end-of-image marker if present.
fn find_jpeg_eoi(buf: &[u8]) -> Option<usize> {
    buf.windows(2)
        .position(|w| w == [0xFF, 0xD9])
        .map(|pos| pos + 2)
}

async fn describe_image(base_url: &str, model: &str, img: &[u8]) -> anyhow::Result<String> {
    let ollama = Ollama::try_new(base_url)?;
    let b64 = general_purpose::STANDARD.encode(img);
    let req = GenerationRequest::new(model.to_string(), PROMPT.to_string())
        .images(vec![Image::from_base64(b64)]);
    trace!(prompt = %PROMPT, "llm prompt");
    let mut stream: GenerationResponseStream = ollama.generate_stream(req).await?;
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(responses) => {
                for resp in responses {
                    trace!(token = %resp.response, "stream token");
                    out.push_str(&resp.response);
                }
            }
            Err(e) => {
                error!(?e, "stream error");
                break;
            }
        }
    }
    let trimmed = out.trim().to_string();
    debug!(response = %trimmed, "llm response");
    Ok(trimmed)
}

async fn caption_loop(
    base_url: String,
    model: String,
    queue: ImageQueue,
    tx: broadcast::Sender<String>,
) {
    loop {
        let img = loop {
            if let Some(i) = queue.pop().await {
                break i;
            }
            queue.wait().await;
        };
        match describe_image(&base_url, &model, &img).await {
            Ok(desc) => {
                debug!(%desc, "caption ready");
                let _ = tx.send(desc);
            }
            Err(e) => error!(?e, "failed to caption image"),
        }
    }
}

async fn handle_connection(
    stream: UnixStream,
    queue: ImageQueue,
    tx: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let mut rx = tx.subscribe();

    let q = queue.clone();
    let mut read_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => {
                    if !buf.is_empty() {
                        q.push(buf).await;
                    }
                    break;
                }
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(end) = find_jpeg_eoi(&buf) {
                        let img = buf.drain(..end).collect();
                        q.push(img).await;
                    }
                }
                Err(_) => break,
            }
        }
    });

    loop {
        tokio::select! {
            _ = &mut read_task => {
                break;
            }
            msg = rx.recv() => match msg {
                Ok(desc) => {
                    if writer.write_all(desc.as_bytes()).await.is_err() { break; }
                    if writer.write_all(b"\n").await.is_err() { break; }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }

    Ok(())
}

/// Run the seen daemon.
pub async fn run(socket: PathBuf, base_url: String, model: String) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "seen listening");

    let queue = ImageQueue::new();
    let (tx, _) = broadcast::channel(8);

    tokio::spawn(caption_loop(
        base_url.clone(),
        model.clone(),
        queue.clone(),
        tx.clone(),
    ));

    loop {
        let (stream, _) = listener.accept().await?;
        let q = queue.clone();
        let t = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, q, t).await {
                error!(?e, "connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use tempfile::tempdir;
    use tokio::io::AsyncBufReadExt;
    use tokio::net::UnixStream;
    use tokio::task::LocalSet;

    #[tokio::test]
    async fn describe_image_sends_request() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"gemma3n\",\"created_at\":\"now\",\"response\":\"desc\",\"done\":true}\n";
        let expected = base64::engine::general_purpose::STANDARD.encode(b"data");
        let mock = server
            .mock_async(move |when, then| {
                when.method(POST)
                    .path("/api/generate")
                    .body_contains("\"images\"")
                    .body_contains(&expected);
                then.status(200)
                    .header("content-type", "application/json")
                    .body(body);
            })
            .await;
        let out = describe_image(&server.base_url(), "gemma3n", b"data")
            .await
            .unwrap();
        assert_eq!(out, "desc");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn run_broadcasts_caption() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"gemma3n\",\"created_at\":\"now\",\"response\":\"a cat\",\"done\":true}\n";
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
        let local = LocalSet::new();
        let run_fut = local.spawn_local(run(sock.clone(), url, "gemma3n".into()));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                // watcher
                let watcher = UnixStream::connect(&sock).await.unwrap();
                let read = tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(watcher);
                    let mut buf = String::new();
                    reader.read_line(&mut buf).await.unwrap();
                    buf
                });
                // sender
                let mut sender = UnixStream::connect(&sock).await.unwrap();
                sender.write_all(b"PNGdata").await.unwrap();
                sender.shutdown().await.unwrap();
                let caption = tokio::time::timeout(std::time::Duration::from_millis(200), read)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(caption.trim(), "a cat");
            })
            .await;
        run_fut.abort();
    }

    #[tokio::test]
    async fn processes_on_jpeg_eoi() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"gemma3n\",\"created_at\":\"now\",\"response\":\"a cat\",\"done\":true}\n";
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
        let local = LocalSet::new();
        let run_fut = local.spawn_local(run(sock.clone(), url, "gemma3n".into()));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let watcher = UnixStream::connect(&sock).await.unwrap();
                let read = tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(watcher);
                    let mut buf = String::new();
                    reader.read_line(&mut buf).await.unwrap();
                    buf
                });
                let mut sender = UnixStream::connect(&sock).await.unwrap();
                sender.write_all(b"JPEGDATA\xFF\xD9").await.unwrap();
                let caption = tokio::time::timeout(std::time::Duration::from_millis(200), read)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(caption.trim(), "a cat");
                drop(sender);
            })
            .await;
        run_fut.abort();
    }

    #[tokio::test]
    async fn handles_timestamp_prefix() {
        let server = MockServer::start_async().await;
        let body =
            "{\"model\":\"gemma3n\",\"created_at\":\"now\",\"response\":\"a cat\",\"done\":true}\n";
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
        let local = LocalSet::new();
        let run_fut = local.spawn_local(run(sock.clone(), url, "gemma3n".into()));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let watcher = UnixStream::connect(&sock).await.unwrap();
                let read = tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(watcher);
                    let mut buf = String::new();
                    reader.read_line(&mut buf).await.unwrap();
                    buf
                });
                let mut sender = UnixStream::connect(&sock).await.unwrap();
                sender
                    .write_all(b"@{2025-07-31T14:00:00Z}\nJPEGDATA\xFF\xD9")
                    .await
                    .unwrap();
                let caption = tokio::time::timeout(std::time::Duration::from_millis(200), read)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(caption.trim(), "a cat");
            })
            .await;
        run_fut.abort();
    }
}
