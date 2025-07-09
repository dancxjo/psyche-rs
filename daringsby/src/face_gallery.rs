use axum::{Json, Router, routing::get};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use psyche_rs::NeoQdrantMemoryStore;

/// Face entry returned by [`FaceGallery`].
#[derive(Serialize)]
pub struct Face {
    /// Unique face identifier.
    pub uuid: String,
    /// Display name associated with the face.
    pub name: String,
    /// URL of the cropped face image.
    pub image_url: String,
}

/// HTTP service exposing known faces from Neo4j.
///
/// # Example
/// ```ignore
/// # use daringsby::face_gallery::FaceGallery;
/// # use psyche_rs::NeoQdrantMemoryStore;
/// # use std::sync::Arc;
/// let store = Arc::new(NeoQdrantMemoryStore::new(
///     "http://localhost:7474",
///     "neo4j",
///     "pass",
///     "http://localhost:6333",
///     llm,
/// ));
/// let gallery = Arc::new(FaceGallery::new(store));
/// let router = gallery.router();
/// ```
#[derive(Clone)]
pub struct FaceGallery {
    store: Arc<NeoQdrantMemoryStore>,
}

impl FaceGallery {
    /// Create a new gallery backed by the given memory store.
    pub fn new(store: Arc<NeoQdrantMemoryStore>) -> Self {
        Self { store }
    }

    /// Build a router exposing `/faces.json`.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route(
            "/faces.json",
            get(move || {
                let this = self.clone();
                async move { Json(this.fetch_faces().await.unwrap_or_default()) }
            }),
        )
    }

    async fn fetch_faces(&self) -> anyhow::Result<Vec<Face>> {
        let query = r#"
MATCH (f:FaceEmbedding)-[:REPRESENTS]->(p:Person)
RETURN f.uuid, f.image_url, p.name ORDER BY p.name
"#;
        let payload = json!({"statements":[{"statement":query,"parameters":{}}]});
        let resp = self
            .store
            .client()
            .post(format!("{}/db/neo4j/tx/commit", self.store.neo4j_url()))
            .basic_auth(self.store.neo_user(), Some(self.store.neo_pass()))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!(status=%resp.status(), "neo4j faces query failed");
            return Ok(Vec::new());
        }
        #[derive(serde::Deserialize)]
        struct R {
            results: Vec<R1>,
        }
        #[derive(serde::Deserialize)]
        struct R1 {
            data: Vec<R2>,
        }
        #[derive(serde::Deserialize)]
        struct R2 {
            row: (String, String, String),
        }
        let res: R = resp.json().await?;
        Ok(res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .map(|r| Face {
                uuid: r.row.0,
                image_url: r.row.1,
                name: r.row.2,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use futures::{StreamExt, stream};
    use httpmock::prelude::*;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct StaticLLM;

    #[async_trait::async_trait]
    impl psyche_rs::LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ollama_rs::generation::chat::ChatMessage],
        ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Box::pin(stream::empty().boxed()))
        }

        async fn embed(
            &self,
            _text: &str,
        ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![0.0])
        }
    }

    #[tokio::test]
    async fn fetch_faces_queries_neo4j() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/db/neo4j/tx/commit");
            then.status(200).json_body(json!({
                "results": [{"data": [{"row": ["f1", "url", "Alice"]}]}]
            }));
        });
        let store = Arc::new(NeoQdrantMemoryStore::new(
            &server.url(""),
            "u",
            "p",
            "http://q",
            Arc::new(StaticLLM),
        ));
        let gallery = FaceGallery::new(store);
        let faces = gallery.fetch_faces().await.unwrap();
        mock.assert();
        assert_eq!(faces[0].name, "Alice");
    }

    #[tokio::test]
    async fn router_serves_json() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({
                "results": [{"data": [{"row": ["f1", "url", "Bob"]}]}]
            }));
        });
        let store = Arc::new(NeoQdrantMemoryStore::new(
            &server.url(""),
            "u",
            "p",
            "http://q",
            Arc::new(StaticLLM),
        ));
        let router = Arc::new(FaceGallery::new(store)).router();
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/faces.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }
}
