use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub async fn memorize(socket: &PathBuf, kind: &str, data: Value) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket).await?;
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "memorize",
        "params": { "kind": kind, "data": data },
        "id": 1
    });
    let msg = serde_json::to_vec(&req)?;
    stream.write_all(&msg).await?;
    stream.shutdown().await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(())
}

pub async fn list(socket: &PathBuf, kind: &str) -> anyhow::Result<Vec<Value>> {
    let mut stream = UnixStream::connect(socket).await?;
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "list",
        "params": { "kind": kind },
        "id": 1
    });
    let msg = serde_json::to_vec(&req)?;
    stream.write_all(&msg).await?;
    stream.shutdown().await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let resp: serde_json::Value = serde_json::from_slice(&buf)?;
    if let Some(list) = resp.get("result").and_then(|v| v.as_array()) {
        Ok(list.clone())
    } else {
        Ok(Vec::new())
    }
}

pub async fn query_vector(
    socket: &PathBuf,
    kind: &str,
    vector: &[f32],
    top_k: usize,
) -> anyhow::Result<Vec<Value>> {
    let mut stream = UnixStream::connect(socket).await?;
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "query_vector",
        "params": { "kind": kind, "vector": vector, "top_k": top_k },
        "id": 1
    });
    let msg = serde_json::to_vec(&req)?;
    stream.write_all(&msg).await?;
    stream.shutdown().await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let resp: serde_json::Value = serde_json::from_slice(&buf)?;
    if let Some(list) = resp.get("result").and_then(|v| v.as_array()) {
        Ok(list.clone())
    } else {
        Ok(Vec::new())
    }
}
