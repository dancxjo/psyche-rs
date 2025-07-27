use serde_json::Value;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tracing::trace;

use crate::policy::Policy;

/// Simple JSONL store used by `rememberd`.
#[derive(Clone)]
pub struct FileStore {
    pub dir: PathBuf,
    policy: Policy,
}

impl FileStore {
    /// Create a new store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Self {
        let policy = Policy::load(&dir);
        Self { dir, policy }
    }

    /// Append a serialized value under the provided memory `kind`.
    pub async fn append(&self, kind: &str, value: &Value) -> anyhow::Result<()> {
        self.write(kind, value).await?;
        if self.policy.recall_for(kind) {
            if let Some(how) = value.get("how").and_then(|v| v.as_str()) {
                if let Some(id) = value.get("id") {
                    let recall = serde_json::json!({
                        "id": uuid::Uuid::new_v4(),
                        "kind": "recall",
                        "when": chrono::Utc::now(),
                        "how": how,
                        "what": [id.clone()],
                    });
                    self.write("recall", &recall).await?;
                }
            }
        }
        Ok(())
    }

    async fn write(&self, kind: &str, value: &Value) -> anyhow::Result<()> {
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
