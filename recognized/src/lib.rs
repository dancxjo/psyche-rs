pub mod memory;
pub mod opencv_recognizer;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, trace};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FaceEntry {
    pub id: Uuid,
    pub embedding: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[async_trait]
pub trait Recognizer: Send + Sync {
    async fn recognize(&self, img: &[u8]) -> anyhow::Result<Vec<String>>;
}

async fn handle_connection(
    stream: UnixStream,
    recognizer: Arc<dyn Recognizer>,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).await?;
    if buf.is_empty() {
        return Ok(());
    }
    let names = recognizer.recognize(&buf).await?;
    let line = if names.is_empty() {
        "(no faces detected)".to_string()
    } else {
        names.join(", ")
    };
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

/// Run the face recognition daemon.
pub async fn run(socket: PathBuf, recognizer: Arc<dyn Recognizer>) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "recognized listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let rec = recognizer.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, rec).await {
                error!(?e, "connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::net::UnixStream;
    use tokio::task::LocalSet;

    struct MockRec;

    #[async_trait]
    impl Recognizer for MockRec {
        async fn recognize(&self, _img: &[u8]) -> anyhow::Result<Vec<String>> {
            Ok(vec!["Alice".into(), "Bob".into()])
        }
    }

    #[tokio::test]
    async fn run_sends_names() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("face.sock");
        let rec = Arc::new(MockRec);
        let local = LocalSet::new();
        let handle = local.spawn_local(run(sock.clone(), rec));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut s = UnixStream::connect(&sock).await.unwrap();
                s.write_all(b"JPEGDATA").await.unwrap();
                s.shutdown().await.unwrap();
                let mut buf = String::new();
                s.read_to_string(&mut buf).await.unwrap();
                assert_eq!(buf.trim(), "Alice, Bob");
            })
            .await;
        handle.abort();
    }
}
