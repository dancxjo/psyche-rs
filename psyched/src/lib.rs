use anyhow::Result;
use psyche::distiller::distill;
use psyche::models::Sensation;
use serde::Serialize;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use uuid::Uuid;

async fn handle_stream(stream: UnixStream, memory: PathBuf) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut path = String::new();
    reader.read_line(&mut path).await?;
    let mut text = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim_end() == "---" {
            break;
        }
        text.push_str(&line);
    }
    let text = text.trim_end_matches('\n').to_string();
    let sensation = Sensation {
        id: Uuid::new_v4().to_string(),
        path: path.trim().to_string(),
        text,
    };
    append(&memory, &sensation).await?;
    if let Some(instant) = distill(&sensation) {
        append(&memory, &instant).await?;
    }
    Ok(())
}

async fn append<T: Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    let line = serde_json::to_string(value)?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    Ok(())
}

/// Runs the psyched daemon until `shutdown` is triggered.
pub async fn run(
    socket: PathBuf,
    memory: PathBuf,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Ok((stream, _)) = listener.accept() => {
                handle_stream(stream, memory.clone()).await?;
            }
        }
    }
    Ok(())
}
