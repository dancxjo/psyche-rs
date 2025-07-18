use std::path::PathBuf;

use psyche::llm::{CanEmbed, LlmProfile};
use psyche::memory::{Experience, MemoryBackend};
use psyche::models::Sensation;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info};
use uuid::Uuid;

/// Handle an incoming recall request.
async fn handle_connection<B: MemoryBackend + Sync>(
    stream: UnixStream,
    out: PathBuf,
    backend: &B,
    embed: &dyn CanEmbed,
    profile: &LlmProfile,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut kind = String::new();
    if reader.read_line(&mut kind).await? == 0 {
        return Ok(());
    }
    let mut query = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        if line.trim_end().is_empty() {
            break;
        }
        query.push_str(&line);
    }
    let query = query.trim().to_string();
    info!(%kind, %query, "recall query");
    let vector = embed.embed(profile, &query).await?;
    let mut results = backend.search(&vector, 5).await?;
    results.retain(passes_filter);
    for exp in results {
        send_sensation(&out, &exp).await?;
    }
    Ok(())
}

fn passes_filter(_e: &Experience) -> bool {
    true
}

async fn send_sensation(socket: &PathBuf, exp: &Experience) -> anyhow::Result<()> {
    let sens = Sensation {
        id: Uuid::new_v4().to_string(),
        path: "/memory/recalled".into(),
        text: exp.how.clone(),
    };
    let data = json!({ "what": exp.what, "tags": exp.tags });
    let mut stream = UnixStream::connect(socket).await?;
    let text = serde_json::to_string(&data)?;
    stream
        .write_all(format!("{}\n{}\n---\n", sens.path, text).as_bytes())
        .await?;
    debug!(?sens.path, len = text.len(), "sent recalled memory");
    Ok(())
}

/// Run the recall daemon.
pub async fn run<B>(
    socket: PathBuf,
    output: PathBuf,
    backend: B,
    embed: Box<dyn CanEmbed>,
    profile: LlmProfile,
) -> anyhow::Result<()>
where
    B: MemoryBackend + Sync + Send + 'static,
{
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "rememberd listening");
    let backend = std::sync::Arc::new(backend);
    let embed = std::sync::Arc::new(embed);
    let profile = std::sync::Arc::new(profile);
    loop {
        let (stream, _) = listener.accept().await?;
        let out = output.clone();
        let b = backend.clone();
        let e = embed.clone();
        let p = profile.clone();
        if let Err(e) = handle_connection(stream, out, b.as_ref(), &**e, &p).await {
            error!(error = %e, "connection failed");
        }
    }
}
