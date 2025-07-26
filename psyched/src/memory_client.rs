use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Client for communicating with `rememberd`.
#[derive(Clone)]
pub struct MemoryClient {
    socket: PathBuf,
}

impl MemoryClient {
    /// Create a new client for the given Unix socket.
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    async fn send(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": uuid::Uuid::new_v4().to_string(),
        });
        let mut stream = UnixStream::connect(&self.socket).await?;
        let data = serde_json::to_vec(&req)?;
        stream.write_all(&data).await?;
        stream.shutdown().await?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await?;
        let resp: serde_json::Value = serde_json::from_slice(&buf)?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!(err.to_string());
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Store a value under the given memory `kind`.
    pub async fn memorize(&self, kind: &str, data: Value) -> anyhow::Result<()> {
        let params = serde_json::json!({"kind": kind, "data": data});
        self.send("memorize", params).await.map(|_| ())
    }

    /// Ping the remote daemon.
    pub async fn ping(&self) -> anyhow::Result<()> {
        self.send("ping", Value::Null).await.map(|_| ())
    }
}
