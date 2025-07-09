use std::{sync::Arc, time::Duration};

use anyhow::Result;
use qdrant_client::{
    Qdrant,
    qdrant::{self, RetrievedPoint, ScrollPointsBuilder, point_id},
};
use serde::Serialize;
use smartcore::{
    cluster::kmeans::{KMeans, KMeansParameters},
    decomposition::pca::{PCA, PCAParameters},
    linalg::basic::{arrays::Array, matrix::DenseMatrix},
};
use tokio::time::interval;
use tracing::{info, warn};

use psyche_rs::{AbortGuard, NeoQdrantMemoryStore};

#[derive(Serialize)]
struct NodeData {
    uuid: String,
    how: String,
    pos: [f32; 3],
    cluster: usize,
}

/// Periodically projects Qdrant embeddings to 3D coordinates and stores them in Neo4j.
///
/// This replaces the previous Python implementation used by `project_embeddings.py`.
pub struct MemoryProjectionService {
    qdrant: Qdrant,
    store: Arc<NeoQdrantMemoryStore>,
    interval: Duration,
}

impl MemoryProjectionService {
    /// Create a new service.
    pub fn new(qdrant: Qdrant, store: Arc<NeoQdrantMemoryStore>, interval: Duration) -> Self {
        Self {
            qdrant,
            store,
            interval,
        }
    }

    /// Spawn the projection loop in the background.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(self) {
        let mut ticker = interval(self.interval);
        loop {
            ticker.tick().await;
            if let Err(e) = self.run_once().await {
                warn!(error=?e, "memory projection failed");
            }
        }
    }

    async fn run_once(&self) -> Result<()> {
        let points = self.fetch_points().await?;
        if points.is_empty() {
            return Ok(());
        }
        let mut uuids = Vec::new();
        let mut hows = Vec::new();
        let mut vectors = Vec::new();
        for p in points {
            let id = match p.id.unwrap().point_id_options.unwrap() {
                point_id::PointIdOptions::Uuid(u) => u,
                point_id::PointIdOptions::Num(n) => n.to_string(),
            };
            let vec = match p.vectors.unwrap().vectors_options.unwrap() {
                qdrant::vectors_output::VectorsOptions::Vector(v) => v.data,
                qdrant::vectors_output::VectorsOptions::Vectors(nv) => nv
                    .vectors
                    .into_iter()
                    .next()
                    .map(|(_, d)| d.data)
                    .unwrap_or_default(),
            };
            let how = p
                .payload
                .get("how")
                .and_then(|v| v.as_str())
                .map_or(String::new(), |v| v.to_string());
            uuids.push(id);
            hows.push(how);
            vectors.push(vec);
        }

        let (positions, labels) = project_embeddings(&vectors)?;
        let entries: Vec<NodeData> = uuids
            .into_iter()
            .zip(hows)
            .zip(positions)
            .zip(labels)
            .map(|(((uuid, how), pos), cluster)| NodeData {
                uuid,
                how,
                pos,
                cluster,
            })
            .collect();

        self.update_neo4j(&entries).await?;
        let data = serde_json::to_vec(&entries)?;
        tokio::fs::create_dir_all("static").await?;
        tokio::fs::write("static/memory_graph.json", data).await?;
        info!("memory projection updated");
        Ok(())
    }

    async fn fetch_points(&self) -> Result<Vec<RetrievedPoint>> {
        let mut out = Vec::new();
        let mut offset = None;
        loop {
            let mut builder = ScrollPointsBuilder::new("impressions")
                .limit(256)
                .with_payload(true)
                .with_vectors(true);
            if let Some(o) = offset.clone() {
                builder = builder.offset(o);
            }
            let res = self.qdrant.scroll(builder).await?;
            out.extend(res.result);
            if let Some(next) = res.next_page_offset {
                offset = Some(next);
            } else {
                break;
            }
        }
        Ok(out)
    }

    async fn update_neo4j(&self, entries: &[NodeData]) -> Result<()> {
        for e in entries {
            let query =
                "MATCH (i:Impression {uuid: $id}) SET i.embedding3d=$pos, i.cluster=$cluster";
            let params = serde_json::json!({
                "id": e.uuid,
                "pos": e.pos,
                "cluster": e.cluster as i32,
            });
            let payload =
                serde_json::json!({"statements":[{"statement":query,"parameters":params}]});
            let resp = self
                .store
                .client()
                .post(format!("{}/db/neo4j/tx/commit", self.store.neo4j_url()))
                .basic_auth(self.store.neo_user(), Some(self.store.neo_pass()))
                .json(&payload)
                .send()
                .await?;
            if !resp.status().is_success() {
                warn!(status=%resp.status(), "neo4j update failed");
            }
        }
        Ok(())
    }
}

/// Reduce vectors to 3D with PCA and cluster with k-means.
fn project_embeddings(vectors: &[Vec<f32>]) -> Result<(Vec<[f32; 3]>, Vec<usize>)> {
    let data: Vec<Vec<f64>> = vectors
        .iter()
        .map(|v| v.iter().map(|&x| x as f64).collect())
        .collect();
    let m = DenseMatrix::from_2d_vec(&data)?;
    let pca = PCA::fit(&m, PCAParameters::default().with_n_components(3))?;
    let reduced = pca.transform(&m)?;

    let kmeans = KMeans::<f64, usize, DenseMatrix<f64>, Vec<usize>>::fit(
        &reduced,
        KMeansParameters::default().with_k(8),
    )?;
    let labels = kmeans.predict(&reduced)?;

    let mut positions = Vec::new();
    for i in 0..reduced.shape().0 {
        positions.push([
            *reduced.get((i, 0)) as f32,
            *reduced.get((i, 1)) as f32,
            *reduced.get((i, 2)) as f32,
        ]);
    }
    let labels_vec = (0..labels.shape()).map(|i| *labels.get(i)).collect();
    Ok((positions, labels_vec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_and_clusters() {
        let data = vec![
            vec![1.0, 0.0, 0.0, 0.0],
            vec![0.9, 0.1, 0.0, 0.0],
            vec![0.0, 1.0, 0.0, 0.0],
            vec![0.0, 0.9, 0.1, 0.0],
        ];
        let (pos, labels) = project_embeddings(&data).unwrap();
        assert_eq!(pos.len(), 4);
        assert_eq!(labels.len(), 4);
    }
}
