use crate::file_memory::FileMemory;
use anyhow::Result;
use chrono::Utc;
use psyche::llm::{CanEmbed, LlmProfile};
use psyche::memory::{Experience, MemoryBackend, QdrantNeo4j};
use psyche::models::{MemoryEntry, Sensation};
use psyche::utils::{first_sentence, parse_json_or_string};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, trace};
use uuid::Uuid;

use crate::memory_client::MemoryClient;

use async_trait::async_trait;

/// Lightweight interface used by pipeline wits to fetch entries.
#[async_trait(?Send)]
pub trait QueryMemory {
    /// Return all entries for the provided kind.
    async fn query_by_kind(&self, kind: &str) -> Result<Vec<MemoryEntry>>;
}

pub struct DbMemory<'a> {
    dir: PathBuf,
    inner: FileMemory,
    backend: Option<Arc<QdrantNeo4j>>,
    embed: &'a dyn CanEmbed,
    profile: &'a LlmProfile,
    db_offsets: Arc<Mutex<HashMap<String, String>>>,
    client: MemoryClient,
}

impl<'a> Clone for DbMemory<'a> {
    fn clone(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            inner: self.inner.clone(),
            backend: self.backend.clone(),
            embed: self.embed,
            profile: self.profile,
            db_offsets: self.db_offsets.clone(),
            client: self.client.clone(),
        }
    }
}

impl<'a> DbMemory<'a> {
    pub fn new(
        dir: PathBuf,
        backend: Option<Arc<QdrantNeo4j>>,
        embed: &'a dyn CanEmbed,
        profile: &'a LlmProfile,
        socket: PathBuf,
    ) -> Self {
        debug!(dir = %dir.display(), "initializing DbMemory");
        Self {
            dir: dir.clone(),
            inner: FileMemory::new(dir),
            backend,
            embed,
            profile,
            db_offsets: Arc::new(Mutex::new(HashMap::new())),
            client: MemoryClient::new(socket),
        }
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    pub async fn query_latest(&self, kind: &str) -> Vec<String> {
        trace!(kind, "DbMemory query_latest");
        if let Some(backend) = &self.backend {
            let mut offsets = self.db_offsets.lock().await;
            let since = offsets
                .get(kind)
                .cloned()
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
            let query = format!(
                "MATCH (e:Experience) WHERE '{tag}' IN e.tags AND e.when > '{since}' \
RETURN e.how AS how, e.what AS what, e.when AS when, e.tags AS tags ORDER BY e.when",
                tag = kind,
                since = since
            );
            match backend.cypher_query(&query).await {
                Ok(mut exps) => {
                    if let Some(last) = exps.last() {
                        offsets.insert(kind.to_string(), last.when.to_rfc3339());
                    }
                    return exps
                        .into_iter()
                        .filter_map(|e| {
                            if kind.starts_with("sensation") {
                                e.what.as_str().map(|s| s.to_string())
                            } else if !e.how.is_empty() {
                                Some(e.how)
                            } else {
                                Some(e.what.to_string())
                            }
                        })
                        .collect();
                }
                Err(e) => {
                    tracing::error!(error = %e, "db query_latest failed");
                }
            }
        }
        self.inner.query_latest(kind).await
    }

    /// Retrieve all stored entries for `kind` advancing database offsets.
    pub async fn entries_by_kind(&self, kind: &str) -> Result<Vec<MemoryEntry>> {
        trace!(kind, "DbMemory query_by_kind");
        if let Some(backend) = &self.backend {
            #[cfg(feature = "neo4j")]
            {
                use chrono::{DateTime, Utc};
                use neo4rs::query;
                let mut offsets = self.db_offsets.lock().await;
                let since = offsets
                    .get(kind)
                    .cloned()
                    .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
                let mut rows = backend
                    .graph
                    .execute(
                        query(
                            "MATCH (e:Experience) WHERE $tag IN e.tags AND e.when > $since RETURN e.id AS id, e.how AS how, e.what AS what, e.when AS when ORDER BY e.when",
                        )
                        .param("tag", kind)
                        .param("since", &since),
                    )
                    .await?;
                let mut out = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let id: String = row.get("id")?;
                    let how: String = row.get("how")?;
                    let what: serde_json::Value = row.get("what")?;
                    let when: String = row.get("when")?;
                    let when = DateTime::parse_from_rfc3339(&when)?.with_timezone(&Utc);
                    out.push(MemoryEntry {
                        id: Uuid::parse_str(&id)?,
                        kind: kind.to_string(),
                        when,
                        what,
                        how,
                    });
                }
                if let Some(last) = out.last() {
                    offsets.insert(kind.to_string(), last.when.to_rfc3339());
                }
                return Ok(out);
            }
        }
        Ok(self.inner.query_by_kind(kind).await?)
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
        let mut out = Vec::new();
        for entry in entries {
            self.client
                .memorize(&entry.kind, serde_json::to_value(entry)?)
                .await?;
            out.push(entry.id.to_string());
        }
        Ok(out)
    }

    pub async fn store_sensation(&self, sens: &Sensation) -> Result<String> {
        self.client
            .memorize(
                &format!("sensation{}", sens.path),
                serde_json::to_value(sens)?,
            )
            .await?;
        Ok(sens.id.clone())
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
            debug!(target = "psyched", ?entry.kind, id = %entry.id, "stored memory entry from wit");
            trace!(id = %entry.id, "persisted entry");
            return Ok(id);
        }
        debug!(target = "psyched", ?entry.kind, id = %entry.id, "stored memory entry from wit");
        trace!(id = %entry.id, "persisted entry");
        Ok(String::new())
    }
}

#[async_trait(?Send)]
impl<'a> QueryMemory for DbMemory<'a> {
    async fn query_by_kind(&self, kind: &str) -> Result<Vec<MemoryEntry>> {
        self.entries_by_kind(kind).await
    }
}
