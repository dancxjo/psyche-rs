use crate::file_memory::FileMemory;
use anyhow::Result;
use chrono::Utc;
use psyche::llm::{CanEmbed, LlmProfile};
use psyche::memory::{Experience, MemoryBackend, QdrantNeo4j};
use psyche::models::{MemoryEntry, Sensation};
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

    pub async fn store(&self, kind: &str, text: &str) -> Result<()> {
        debug!(kind, "DbMemory store");
        let entry = MemoryEntry {
            id: Uuid::new_v4(),
            kind: kind.to_string(),
            when: Utc::now(),
            what: Value::String(text.to_string()),
            how: text.to_string(),
        };
        self.append_entries(&[entry]).await
    }

    pub async fn append_entries(&self, entries: &[MemoryEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
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
        for entry in entries {
            let line = serde_json::to_string(entry)?;
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
            self.persist(entry).await?;
        }
        Ok(())
    }

    pub async fn store_sensation(&self, sens: &Sensation) -> Result<()> {
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
                how: sens.text.clone(),
                what: Value::String(sens.text.clone()),
                when: Utc::now(),
                tags: vec![sens.path.clone()],
            };
            backend.store(&exp, &vector).await?;
        }
        Ok(())
    }

    async fn persist(&self, entry: &MemoryEntry) -> Result<()> {
        if let Some(backend) = &self.backend {
            let vector = self.embed.embed(self.profile, &entry.how).await?;
            let exp = Experience {
                how: entry.how.clone(),
                what: entry.what.clone(),
                when: entry.when,
                tags: vec![entry.kind.clone()],
            };
            backend.store(&exp, &vector).await?;
        }
        trace!(id = %entry.id, "persisted entry");
        Ok(())
    }
}
