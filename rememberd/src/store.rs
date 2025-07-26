use serde_json::Value;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tracing::trace;

/// Simple JSONL store used by `rememberd`.
#[derive(Clone)]
pub struct FileStore {
    pub dir: PathBuf,
}

impl FileStore {
    /// Create a new store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Append a serialized value under the provided memory `kind`.
    pub async fn append(&self, kind: &str, value: &Value) -> anyhow::Result<()> {
        let base = kind.split('/').next().unwrap_or(kind);
        let path = self.dir.join(format!("{}.jsonl", base));
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .await?;
        let line = serde_json::to_string(value)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        trace!(?kind, "stored entry");
        Ok(())
    }
}
