use crate::file_memory::FileMemory;
use anyhow::Result;
use chrono::Utc;
use psyche::llm::{CanEmbed, LlmProfile};
use psyche::memory::{Experience, MemoryBackend, QdrantNeo4j};
use psyche::models::{MemoryEntry, Sensation};
use psyche::utils::{first_sentence, parse_json_or_string};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::{debug, trace};
use uuid::Uuid;

pub struct DbMemory<'a> {
    dir: PathBuf,
    inner: FileMemory,
    backend: Option<Arc<QdrantNeo4j>>,
    embed: &'a dyn CanEmbed,
    profile: &'a LlmProfile,
}

impl<'a> Clone for DbMemory<'a> {
    fn clone(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            inner: self.inner.clone(),
            backend: self.backend.clone(),
            embed: self.embed,
            profile: self.profile,
        }
    }
}

impl<'a> DbMemory<'a> {
    pub fn new(
        dir: PathBuf,
        backend: Option<Arc<QdrantNeo4j>>,
        embed: &'a dyn CanEmbed,
        profile: &'a LlmProfile,
    ) -> Self {
        debug!(dir = %dir.display(), "initializing DbMemory");
        Self {
            dir: dir.clone(),
            inner: FileMemory::new(dir),
            backend,
            embed,
            profile,
        }
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    pub async fn query_latest(&self, kind: &str) -> Vec<String> {
        trace!(kind, "DbMemory query_latest");
        self.inner.query_latest(kind).await
    }

    pub async fn link_summary(&self, summary_id: &str, original_id: &str) -> Result<()> {
        if let Some(backend) = &self.backend {
            backend.link_summary(summary_id, original_id).await?;
        }
        Ok(())
    }

    pub async fn store(&self, kind: &str, text: &str) -> Result<String> {
        debug!(kind, "DbMemory store");
        let entry = MemoryEntry {
            id: Uuid::new_v4(),
            kind: kind.to_string(),
            when: Utc::now(),
            what: parse_json_or_string(text),
            how: first_sentence(text),
        };
        let ids = self.append_entries(&[entry]).await?;
        Ok(ids.first().cloned().unwrap_or_default())
    }

    pub async fn append_entries(&self, entries: &[MemoryEntry]) -> Result<Vec<String>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        debug!(count = entries.len(), "appending entries");
        let path = self.dir.join(format!(
            "{}.jsonl",
            entries[0].kind.split('/').next().unwrap()
        ));
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .await?;
        let mut out = Vec::new();
        for entry in entries {
            let line = serde_json::to_string(entry)?;
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
            let id = self.persist(entry).await?;
            out.push(id);
        }
        Ok(out)
    }

    pub async fn store_sensation(&self, sens: &Sensation) -> Result<String> {
        let path = self.dir.join("sensation.jsonl");
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .await?;
        let line = serde_json::to_string(sens)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        trace!("stored sensation to file");
        if let Some(backend) = &self.backend {
            let vector = self.embed.embed(self.profile, &sens.text).await?;
            let exp = Experience {
                how: first_sentence(&sens.text),
                what: parse_json_or_string(&sens.text),
                when: Utc::now(),
                tags: vec![sens.path.clone()],
            };
            let id = backend.store(&exp, &vector).await?;
            return Ok(id);
        }
        Ok(String::new())
    }

    async fn persist(&self, entry: &MemoryEntry) -> Result<String> {
        if let Some(backend) = &self.backend {
            let vector = self.embed.embed(self.profile, &entry.how).await?;
            let exp = Experience {
                how: entry.how.clone(),
                what: entry.what.clone(),
                when: entry.when,
                tags: vec![entry.kind.clone()],
            };
            let id = backend.store(&exp, &vector).await?;
            if let Value::Array(arr) = &entry.what {
                for v in arr {
                    if let Some(pid) = v.as_str() {
                        let _ = backend.link_summary(&id, pid).await;
                    }
                }
            }
            trace!(id = %entry.id, "persisted entry");
            return Ok(id);
        }
        trace!(id = %entry.id, "persisted entry");
        Ok(String::new())
    }
}
