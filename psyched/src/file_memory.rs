use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::Mutex;

use crate::db_memory::QueryMemory;
use async_trait::async_trait;
use chrono::Utc;
use psyche::models::{MemoryEntry, Sensation};
use psyche::utils::{first_sentence, parse_json_or_string};
use serde_json::Value;
use tracing::{debug, trace};
use uuid::Uuid;

/// Simple JSONL-backed memory store for Wit scheduling.
#[derive(Clone)]
pub struct FileMemory {
    dir: PathBuf,
    offsets: Arc<Mutex<HashMap<String, usize>>>,
}

#[async_trait(?Send)]
impl QueryMemory for FileMemory {
    async fn query_by_kind(&self, kind: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        self.entries_by_kind(kind).await
    }
}

use std::sync::Arc;

impl FileMemory {
    /// Create a new memory store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Self {
        debug!(dir = %dir.display(), "creating FileMemory");
        Self {
            dir,
            offsets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Return newly appended items for `kind` since the last call.
    pub async fn query_latest(&self, kind: &str) -> Vec<String> {
        trace!(kind, "query_latest called");
        let path = self
            .dir
            .join(format!("{}.jsonl", kind.split('/').next().unwrap_or(kind)));
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let lines: Vec<_> = content.lines().collect();
        let mut offsets = self.offsets.lock().await;
        let start = offsets.entry(kind.to_string()).or_insert(0);
        let slice = if *start < lines.len() {
            &lines[*start..]
        } else {
            &[]
        };
        *start = lines.len();
        slice
            .iter()
            .filter_map(|l| {
                if kind.starts_with("sensation") {
                    serde_json::from_str::<Sensation>(l).ok().and_then(|s| {
                        let entry_kind = format!("sensation{}", s.path);
                        if entry_kind.starts_with(kind) {
                            Some(s.text)
                        } else {
                            None
                        }
                    })
                } else {
                    serde_json::from_str::<MemoryEntry>(l).ok().map(|e| {
                        if !e.how.is_empty() {
                            e.how
                        } else {
                            e.what.to_string()
                        }
                    })
                }
            })
            .collect()
    }

    /// Return all entries for `kind` without advancing internal offsets.
    pub async fn entries_by_kind(&self, kind: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        trace!(kind, "query_by_kind called");
        let path = self
            .dir
            .join(format!("{}.jsonl", kind.split('/').next().unwrap_or(kind)));
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let mut out = Vec::new();
        for line in content.lines() {
            if kind.starts_with("sensation") {
                if let Ok(s) = serde_json::from_str::<Sensation>(line) {
                    let entry_kind = format!("sensation{}", s.path);
                    if entry_kind.starts_with(kind) {
                        out.push(MemoryEntry {
                            id: Uuid::parse_str(&s.id)?,
                            kind: entry_kind,
                            when: Utc::now(),
                            what: serde_json::json!(s.text),
                            how: String::new(),
                        });
                    }
                }
            } else if let Ok(mut e) = serde_json::from_str::<MemoryEntry>(line) {
                e.kind = kind.to_string();
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Append a new text value under the given `kind`.
    pub async fn store(&self, kind: &str, text: &str) -> anyhow::Result<()> {
        debug!(kind, "storing entry");
        let entry = MemoryEntry {
            id: Uuid::new_v4(),
            kind: kind.to_string(),
            when: Utc::now(),
            what: parse_json_or_string(text),
            how: first_sentence(text),
        };
        self.append(kind, &entry).await
    }

    async fn append(&self, kind: &str, value: &MemoryEntry) -> anyhow::Result<()> {
        let path = self.dir.join(format!("{}.jsonl", kind));
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .await?;
        let line = serde_json::to_string(value)?;
        use tokio::io::AsyncWriteExt;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        trace!(kind, "appended entry");
        Ok(())
    }
}
