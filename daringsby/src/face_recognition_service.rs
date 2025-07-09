use chrono::Local;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::{broadcast::Receiver, mpsc::UnboundedSender};
use tracing::warn;
use url::Url;

use psyche_rs::{AbortGuard, Sensation};

/// Service that recognizes faces by searching embeddings in Qdrant.
///
/// # Example
/// ```no_run
/// # use daringsby::face_recognition_service::FaceRecognitionService;
/// # use psyche_rs::Sensation;
/// # use reqwest::Client;
/// # use serde_json::json;
/// # use tokio::sync::{broadcast, mpsc};
/// # use url::Url;
/// # use chrono::Local;
/// # use serde_json::Value;
/// let (tx, rx) = broadcast::channel(1);
/// let (out_tx, _out_rx) = mpsc::unbounded_channel();
/// let service = FaceRecognitionService::new(
///     rx,
///     out_tx,
///     Client::new(),
///     Url::parse("http://localhost:6333").unwrap(),
///     0.9,
///     Url::parse("http://localhost:7474").unwrap(),
///     "neo4j",
///     "pass",
/// );
/// let _guard = service.spawn();
/// # let _ = tx.send(vec![Sensation::<Value>{kind:"face.embedding".into(), when:Local::now(), what:json!({"face_id":"f1","embedding":vec![0.0_f32; 512]}), source:None}]);
/// ```
pub struct FaceRecognitionService {
    rx: Receiver<Vec<Sensation<Value>>>,
    tx: UnboundedSender<Vec<Sensation<String>>>,
    client: Client,
    qdrant_url: Url,
    threshold: f32,
    neo4j_url: Url,
    neo_user: String,
    neo_pass: String,
}

impl FaceRecognitionService {
    /// Create a new service.
    pub fn new(
        rx: Receiver<Vec<Sensation<Value>>>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
        client: Client,
        qdrant_url: Url,
        threshold: f32,
        neo4j_url: Url,
        neo_user: impl Into<String>,
        neo_pass: impl Into<String>,
    ) -> Self {
        Self {
            rx,
            tx,
            client,
            qdrant_url,
            threshold,
            neo4j_url,
            neo_user: neo_user.into(),
            neo_pass: neo_pass.into(),
        }
    }

    /// Spawn the recognition loop.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(mut self) {
        while let Ok(batch) = self.rx.recv().await {
            for sens in batch {
                if sens.kind == "face.embedding" {
                    if let Err(e) = self.handle_embedding(&sens).await {
                        warn!(error=?e, "face recognition failed");
                    }
                }
            }
        }
    }

    async fn handle_embedding(&self, sens: &Sensation<Value>) -> anyhow::Result<()> {
        let embedding = sens
            .what
            .get("embedding")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing embedding"))?;
        let vector: Vec<f32> = embedding
            .iter()
            .filter_map(|v| v.as_f64())
            .map(|f| f as f32)
            .collect();
        let face_id = sens
            .what
            .get("face_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing face_id"))?;
        let body = json!({"vector": vector, "limit": 2, "with_payload": false});
        let url = self
            .qdrant_url
            .join("collections/face_embeddings/points/search")?;
        let resp = self.client.post(url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            warn!(%status, %text, "qdrant search failed");
            return Ok(());
        }
        #[derive(serde::Deserialize)]
        struct SearchItem {
            id: String,
            score: f32,
        }
        #[derive(serde::Deserialize)]
        struct SearchRes {
            result: Vec<SearchItem>,
        }
        let result: SearchRes = resp.json().await?;
        let matched = result.result.into_iter().filter(|r| r.id != face_id).next();
        if let Some(item) = matched {
            if item.score >= self.threshold {
                let name = self.fetch_name(&item.id).await.ok().flatten();
                let text = name
                    .as_deref()
                    .map(|n| format!("I recognize {}'s face.", n))
                    .unwrap_or_else(|| "I recognized a known face.".to_string());
                let sensation = Sensation {
                    kind: "face.recognized".into(),
                    when: Local::now(),
                    what: text,
                    source: None,
                };
                let _ = self.tx.send(vec![sensation]);
            }
        }
        Ok(())
    }

    async fn fetch_name(&self, face_id: &str) -> anyhow::Result<Option<String>> {
        let query =
            "MATCH (f:FaceEmbedding {uuid:$id})-[:REPRESENTS]->(p:Person) RETURN p.name LIMIT 1";
        let params = json!({"id": face_id});
        let payload = json!({"statements":[{"statement":query,"parameters":params}]});
        let url = self.neo4j_url.join("db/neo4j/tx/commit")?;
        let resp = self
            .client
            .post(url)
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!(status=%resp.status(), "neo4j name query failed");
            return Ok(None);
        }
        #[derive(serde::Deserialize)]
        struct Res {
            results: Vec<R1>,
        }
        #[derive(serde::Deserialize)]
        struct R1 {
            data: Vec<R2>,
        }
        #[derive(serde::Deserialize)]
        struct R2 {
            row: (Option<String>,),
        }
        let res: Res = resp.json().await?;
        Ok(res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .next()
            .and_then(|r| r.row.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[tokio::test]
    async fn recognizes_face_when_similarity_high() {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/collections/face_embeddings/points/search");
            then.status(200).json_body(json!({
                "result": [{"id": "known", "score": 0.95}]
            }));
        });
        let neo = MockServer::start();
        let neo_mock = neo.mock(|when, then| {
            when.method(POST).path("/db/neo4j/tx/commit");
            then.status(200).json_body(json!({
                "results": [{"data": [{"row": ["Travis"]}]}]
            }));
        });
        let service = FaceRecognitionService::new(
            rx,
            out_tx,
            Client::new(),
            Url::parse(&server.url("/")).unwrap(),
            0.9,
            Url::parse(&neo.url("/")).unwrap(),
            "u",
            "p",
        );
        let guard = service.spawn();
        let embedding = vec![0.0_f32; 512];
        let sens = Sensation {
            kind: "face.embedding".into(),
            when: Local::now(),
            what: json!({"face_id": "new", "embedding": embedding}),
            source: None,
        };
        tx.send(vec![sens]).unwrap();
        let out = out_rx.recv().await.unwrap();
        assert_eq!(out[0].kind, "face.recognized");
        assert_eq!(out[0].what, "I recognize Travis's face.");
        mock.assert();
        neo_mock.assert();
        drop(guard);
    }
}
