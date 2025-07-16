use anyhow::Result;
use chrono::Utc;
use psyche::distiller::{Combobulator, Distiller};
use psyche::models::{MemoryEntry, Sensation};
use serde::Serialize;
use serde_json::json;
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

async fn append_all<T: Serialize>(path: &PathBuf, values: &[T]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    for v in values {
        let line = serde_json::to_string(v)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
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
    let instant_path = memory.with_file_name("instant.jsonl");
    let mut combobulator = Combobulator;
    let mut processed = 0usize;
    let mut beat = tokio::time::interval(std::time::Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Ok((stream, _)) = listener.accept() => {
                handle_stream(stream, memory.clone()).await?;
            }
            _ = beat.tick() => {
                let content = tokio::fs::read_to_string(&memory).await.unwrap_or_default();
                let lines: Vec<_> = content.lines().collect();
                if processed < lines.len() {
                    let new_lines = &lines[processed..];
                    processed = lines.len();
                    let mut input = Vec::new();
                    for line in new_lines {
                        let s: Sensation = serde_json::from_str(line)?;
                        input.push(MemoryEntry {
                            id: Uuid::parse_str(&s.id)?,
                            kind: "sensation/chat".to_string(),
                            when: Utc::now(),
                            what: json!(s.text),
                            how: String::new(),
                        });
                    }
                    let out = combobulator.distill(input).await?;
                    append_all(&instant_path, &out).await?;
                }
            }
        }
    }
    Ok(())
}
