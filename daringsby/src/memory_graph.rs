use axum::{Json, Router, routing::get};
use psyche_rs::{MemoryStore, NeoQdrantMemoryStore};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

#[derive(Serialize)]
struct Edge {
    to: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize)]
struct Node {
    uuid: String,
    how: String,
    pos: [f32; 3],
    cluster: Option<i32>,
    edges: Vec<Edge>,
}

/// HTTP endpoint serving a memory graph for visualization.
///
/// Loads impression data and their relationships from Neo4j along with
/// 3D positions stored by `project_embeddings.py`.
#[derive(Clone)]
pub struct MemoryGraph {
    store: Arc<NeoQdrantMemoryStore>,
}

impl MemoryGraph {
    pub fn new(store: Arc<NeoQdrantMemoryStore>) -> Self {
        Self { store }
    }

    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route(
                "/memory_graph.json",
                get(move || {
                    let this = self.clone();
                    async move { Json(this.build_graph().await.unwrap_or_default()) }
                }),
            )
            .route("/memory_viz.html", get(Self::viz))
    }

    async fn viz() -> impl axum::response::IntoResponse {
        const PAGE: &str = include_str!("memory_viz.html");
        axum::response::Html(PAGE)
    }

    async fn build_graph(&self) -> anyhow::Result<Vec<Node>> {
        // load a larger set of recent impressions so more nodes are
        // available inside the WebXR visualization
        let nodes = self.store.fetch_recent_impressions(500).await?;
        let pos_map = load_positions().unwrap_or_default();
        let mut out = Vec::new();
        for imp in nodes {
            let (pos, cluster) = pos_map
                .get(&imp.id)
                .map(|p| (p.pos, Some(p.cluster)))
                .unwrap_or(([0.0, 0.0, 0.0], None));
            let edges = self.fetch_edges(&imp.id).await.unwrap_or_default();
            out.push(Node {
                uuid: imp.id,
                how: imp.how,
                pos,
                cluster,
                edges,
            });
        }
        Ok(out)
    }

    async fn fetch_edges(&self, id: &str) -> anyhow::Result<Vec<Edge>> {
        let query = r#"
MATCH (a:Impression {uuid:$id})-[r:SUMMARIZES]->(b:Impression)
RETURN type(r) AS type, b.uuid AS uuid
"#;
        let params = json!({"id": id});
        let payload = json!({"statements":[{"statement":query,"parameters":params}]});
        let resp = self
            .store
            .client()
            .post(format!("{}/db/neo4j/tx/commit", self.store.neo4j_url()))
            .basic_auth(self.store.neo_user(), Some(self.store.neo_pass()))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!(status = %resp.status(), "neo4j edge query failed");
            return Ok(Vec::new());
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
            row: (String, String),
        }
        let res: Res = resp.json().await?;
        Ok(res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .map(|r| Edge {
                kind: r.row.0,
                to: r.row.1,
            })
            .collect())
    }
}

#[derive(serde::Deserialize)]
struct PosEntry {
    uuid: String,
    pos: [f32; 3],
    cluster: i32,
}

fn load_positions() -> Option<HashMap<String, PosEntry>> {
    let data = std::fs::read_to_string("embedding_positions.json").ok()?;
    let list: Vec<PosEntry> = serde_json::from_str(&data).ok()?;
    Some(list.into_iter().map(|p| (p.uuid.clone(), p)).collect())
}
