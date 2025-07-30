use qdrant_client::prelude::*;
use qdrant_client::qdrant::{
    point_id, CreateCollectionBuilder, Distance, PointStruct, SearchPoints, VectorParamsBuilder,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tracing::trace;

use crate::policy::Policy;

/// Simple JSONL store used by `rememberd`.
#[derive(Clone)]
pub struct FileStore {
    pub dir: PathBuf,
    policy: Policy,
    qdrant: Option<std::sync::Arc<QdrantClient>>,
}

impl FileStore {
    /// Create a new store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Self {
        let policy = Policy::load(&dir);
        Self {
            dir,
            policy,
            qdrant: None,
        }
    }

    pub fn with_qdrant(dir: PathBuf, qdrant: QdrantClient) -> Self {
        let policy = Policy::load(&dir);
        Self {
            dir,
            policy,
            qdrant: Some(std::sync::Arc::new(qdrant)),
        }
    }

    /// Append a serialized value under the provided memory `kind`.
    pub async fn append(&self, kind: &str, value: &Value) -> anyhow::Result<()> {
        if kind == "face" {
            if let Some(client) = &self.qdrant {
                if let (Some(idv), Some(embv)) = (value.get("id"), value.get("embedding")) {
                    if let (Some(id), Some(arr)) = (idv.as_str(), embv.as_array()) {
                        let vector: Vec<f32> = arr
                            .iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect();
                        ensure_faces_collection(&**client, vector.len() as u64).await?;
                        let points = vec![PointStruct::new(
                            id.to_string(),
                            vector,
                            HashMap::<String, qdrant_client::qdrant::Value>::new(),
                        )];
                        client
                            .upsert_points_blocking("faces", None, points, None)
                            .await?;
                    }
                }
            }
        }
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

impl FileStore {
    /// List all entries for a given memory kind.
    pub async fn list(&self, kind: &str) -> anyhow::Result<Vec<Value>> {
        let base = kind.split('/').next().unwrap_or(kind);
        let path = self.dir.join(format!("{}.jsonl", base));
        let mut out = Vec::new();
        if let Ok(data) = tokio::fs::read_to_string(&path).await {
            for line in data.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    out.push(v);
                }
            }
        }
        Ok(out)
    }

    pub async fn query_vector(
        &self,
        kind: &str,
        vector: &[f32],
        top_k: usize,
    ) -> anyhow::Result<Vec<Value>> {
        if kind != "face" {
            return Ok(Vec::new());
        }
        let client = match &self.qdrant {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };
        ensure_faces_collection(client, vector.len() as u64).await?;
        let req = SearchPoints {
            collection_name: "faces".into(),
            vector: vector.to_vec(),
            filter: None,
            limit: top_k as u64,
            with_payload: None,
            params: None,
            score_threshold: None,
            offset: None,
            vector_name: None,
            with_vectors: None,
            read_consistency: None,
            timeout: None,
            shard_key_selector: None,
            sparse_indices: None,
        };
        let res = client.search_points(&req).await?;
        let mut out = Vec::new();
        for pt in res.result {
            if let Some(id) = pt.id.as_ref() {
                if let Some(opt) = id.point_id_options.as_ref() {
                    let pid = match opt {
                        point_id::PointIdOptions::Uuid(u) => u.clone(),
                        point_id::PointIdOptions::Num(n) => n.to_string(),
                    };
                    out.push(serde_json::json!({"id": pid, "score": pt.score}));
                }
            }
        }
        Ok(out)
    }
}

async fn ensure_faces_collection(client: &QdrantClient, dim: u64) -> anyhow::Result<()> {
    if !client.collection_exists("faces").await? {
        let req = CreateCollectionBuilder::new("faces")
            .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine))
            .build();
        client.create_collection(&req).await?;
    }
    Ok(())
}
