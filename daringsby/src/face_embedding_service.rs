use async_trait::async_trait;
use chrono::{Local, Utc};
use reqwest::Client;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast::Receiver, mpsc::UnboundedSender};
use tracing::{debug, warn};
use url::Url;

use psyche_rs::{AbortGuard, Sensation};

/// Resulting face crop and embedding.
pub struct FaceData {
    /// JPEG bytes of the cropped face.
    pub crop: Vec<u8>,
    /// 512-dimensional embedding vector.
    pub embedding: Vec<f32>,
}

/// Trait for face detection and embedding.
#[async_trait]
pub trait FaceEmbedder: Send + Sync {
    /// Detect and embed faces in the provided image bytes.
    async fn detect(&self, image: &[u8]) -> anyhow::Result<Vec<FaceData>>;
}

/// Background service that processes vision snapshots into face embeddings.
pub struct FaceEmbeddingService<E: FaceEmbedder> {
    rx: Receiver<Vec<u8>>, // JPEG snapshots
    tx: UnboundedSender<Vec<Sensation<serde_json::Value>>>,
    embedder: Arc<E>,
    client: Client,
    qdrant_url: Url,
    face_dir: PathBuf,
    base_url: Url,
    neo4j_url: Url,
    neo_user: String,
    neo_pass: String,
}

impl<E: FaceEmbedder + 'static> FaceEmbeddingService<E> {
    /// Create a new service.
    pub fn new(
        rx: Receiver<Vec<u8>>,
        tx: UnboundedSender<Vec<Sensation<serde_json::Value>>>,
        embedder: Arc<E>,
        client: Client,
        qdrant_url: Url,
        face_dir: PathBuf,
        base_url: Url,
        neo4j_url: Url,
        neo_user: impl Into<String>,
        neo_pass: impl Into<String>,
    ) -> Self {
        Self {
            rx,
            tx,
            embedder,
            client,
            qdrant_url,
            face_dir,
            base_url,
            neo4j_url,
            neo_user: neo_user.into(),
            neo_pass: neo_pass.into(),
        }
    }

    /// Spawn the processing loop.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(mut self) {
        while let Ok(img) = self.rx.recv().await {
            let snapshot_uuid = uuid::Uuid::new_v4().to_string();
            match self.embedder.detect(&img).await {
                Ok(faces) => {
                    for (i, face) in faces.into_iter().enumerate() {
                        if let Err(e) = self.handle_face(&snapshot_uuid, i, face).await {
                            warn!(error=?e, "face handling failed");
                        }
                    }
                }
                Err(e) => warn!(error=?e, "face detection failed"),
            }
        }
    }

    async fn handle_face(
        &self,
        snapshot_uuid: &str,
        idx: usize,
        face: FaceData,
    ) -> anyhow::Result<()> {
        let face_id = format!("{snapshot_uuid}_face{idx}");
        let file_name = format!("{face_id}.jpg");
        if !self.face_dir.exists() {
            tokio::fs::create_dir_all(&self.face_dir).await?;
        }
        let path = self.face_dir.join(&file_name);
        tokio::fs::write(&path, &face.crop).await?;
        let url = self.base_url.join(&file_name)?;
        let payload = json!({
            "snapshot_uuid": snapshot_uuid,
            "face_id": face_id,
            "timestamp": Utc::now().to_rfc3339(),
            "image_url": url.as_str(),
        });
        let body = json!({
            "points": [{
                "id": face_id,
                "vector": face.embedding,
                "payload": payload,
            }]
        });
        let ep = self.qdrant_url.join("collections/face_embeddings/points")?;
        let resp = self.client.put(ep).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            warn!(%status, %text, "qdrant insert failed");
        }
        let query = "MERGE (s:VisionSnapshot {uuid:$snap})\nMERGE (f:FaceEmbedding {uuid:$face})\nSET f.image_url=$url\nMERGE (f)-[:DERIVED_FROM]->(s)";
        let params = json!({"snap": snapshot_uuid, "face": face_id, "url": url.as_str()});
        let payload = json!({"statements":[{"statement":query,"parameters":params}]});
        let url_neo = self.neo4j_url.join("db/neo4j/tx/commit")?;
        let resp = self
            .client
            .post(url_neo)
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!(status=%resp.status(), "neo4j face link failed");
        }
        let sensation = Sensation {
            kind: "face.embedding".into(),
            when: Local::now(),
            what: json!({
                "snapshot_uuid": snapshot_uuid,
                "face_crop_url": url.as_str(),
                "embedding": face.embedding,
                "face_id": face_id,
            }),
            source: None,
        };
        let _ = self.tx.send(vec![sensation]);
        debug!(face_id, "face embedding emitted");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use httpmock::prelude::*;
    use tempfile::tempdir;

    struct StubEmbedder;
    #[async_trait]
    impl FaceEmbedder for StubEmbedder {
        async fn detect(&self, _image: &[u8]) -> anyhow::Result<Vec<FaceData>> {
            Ok(vec![FaceData {
                crop: vec![1, 2, 3],
                embedding: vec![0.0; 512],
            }])
        }
    }

    #[tokio::test]
    async fn processes_snapshot() {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        let (sens_tx, mut sens_rx) = tokio::sync::mpsc::unbounded_channel();
        let qdrant = MockServer::start();
        let q_mock = qdrant.mock(|when, then| {
            when.method(PUT);
            then.status(200);
        });
        let neo = MockServer::start();
        let neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200);
        });
        let tmp = tempdir().unwrap();
        let service = FaceEmbeddingService::new(
            rx,
            sens_tx,
            Arc::new(StubEmbedder),
            Client::new(),
            Url::parse(&qdrant.url("/")).unwrap(),
            tmp.path().to_path_buf(),
            Url::parse("http://localhost/faces/").unwrap(),
            Url::parse(&neo.url("/")).unwrap(),
            "u",
            "p",
        );
        let guard = service.spawn();
        tx.send(vec![0]).unwrap();
        let sens = sens_rx.recv().await.unwrap();
        assert_eq!(sens[0].kind, "face.embedding");
        q_mock.assert();
        neo_mock.assert();
        drop(guard);
    }
}
