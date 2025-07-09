use std::{collections::HashSet, time::Duration};

use anyhow::Result;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{self, RetrievedPoint, ScrollPointsBuilder, point_id};
use reqwest::Client;
use smartcore::cluster::kmeans::{KMeans, KMeansParameters};
use smartcore::linalg::basic::{arrays::Array, matrix::DenseMatrix};
use tokio::time::interval;
use tracing::{info, warn};
use url::Url;

use psyche_rs::AbortGuard;

/// Periodically clusters face embeddings that have not been named.
///
/// # Example
/// ```no_run
/// use daringsby::face_clustering_service::FaceClusteringService;
/// use qdrant_client::Qdrant;
/// use url::Url;
/// use reqwest::Client;
/// let service = FaceClusteringService::new(
///     Qdrant::from_url("http://localhost:6333").build().unwrap(),
///     Client::new(),
///     Url::parse("http://localhost:7474").unwrap(),
///     "neo4j",
///     "password",
///     std::time::Duration::from_secs(60),
/// );
/// let _guard = service.spawn();
/// ```
pub struct FaceClusteringService {
    qdrant: Qdrant,
    client: Client,
    neo4j_url: Url,
    neo_user: String,
    neo_pass: String,
    interval: Duration,
}

impl FaceClusteringService {
    /// Create a new service.
    pub fn new(
        qdrant: Qdrant,
        client: Client,
        neo4j_url: Url,
        neo_user: impl Into<String>,
        neo_pass: impl Into<String>,
        interval: Duration,
    ) -> Self {
        Self {
            qdrant,
            client,
            neo4j_url,
            neo_user: neo_user.into(),
            neo_pass: neo_pass.into(),
            interval,
        }
    }

    /// Spawn the clustering loop.
    pub fn spawn(self) -> AbortGuard {
        let handle = tokio::spawn(async move { self.run().await });
        AbortGuard::new(handle)
    }

    async fn run(self) {
        let mut ticker = interval(self.interval);
        loop {
            ticker.tick().await;
            if let Err(e) = self.run_once().await {
                warn!(error=?e, "face clustering failed");
            }
        }
    }

    async fn run_once(&self) -> Result<()> {
        let named = self.fetch_named_faces().await?;
        let points = self.fetch_points().await?;
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for p in points {
            let id = match p.id.unwrap().point_id_options.unwrap() {
                point_id::PointIdOptions::Uuid(u) => u,
                point_id::PointIdOptions::Num(n) => n.to_string(),
            };
            if named.contains(&id) {
                continue;
            }
            let vec = match p.vectors.unwrap().vectors_options.unwrap() {
                qdrant::vectors_output::VectorsOptions::Vector(v) => v.data,
                qdrant::vectors_output::VectorsOptions::Vectors(nv) => nv
                    .vectors
                    .into_iter()
                    .next()
                    .map(|(_, d)| d.data)
                    .unwrap_or_default(),
            };
            ids.push(id);
            vectors.push(vec);
        }
        if ids.is_empty() {
            return Ok(());
        }
        let labels = cluster_vectors(&vectors)?;
        self.update_neo4j(&ids, &labels).await?;
        info!("face clustering updated");
        Ok(())
    }

    async fn fetch_named_faces(&self) -> Result<HashSet<String>> {
        let query = "MATCH (f:FaceEmbedding) WHERE exists(f.name) RETURN f.uuid";
        let payload = serde_json::json!({"statements":[{"statement":query} ]});
        let resp = self
            .client
            .post(self.neo4j_url.join("db/neo4j/tx/commit")?)
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!(status=%resp.status(), "neo4j query failed");
            return Ok(HashSet::new());
        }
        #[derive(serde::Deserialize)]
        struct NeoRes {
            results: Vec<R1>,
        }
        #[derive(serde::Deserialize)]
        struct R1 {
            data: Vec<R2>,
        }
        #[derive(serde::Deserialize)]
        struct R2 {
            row: (String,),
        }
        let res: NeoRes = resp.json().await?;
        Ok(res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .map(|r| r.row.0)
            .collect())
    }

    async fn fetch_points(&self) -> Result<Vec<RetrievedPoint>> {
        let mut out = Vec::new();
        let mut offset = None;
        loop {
            let mut builder = ScrollPointsBuilder::new("face_embeddings")
                .limit(256)
                .with_vectors(true)
                .with_payload(true);
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

    async fn update_neo4j(&self, ids: &[String], labels: &[usize]) -> Result<()> {
        for (id, cluster) in ids.iter().zip(labels) {
            let query = "MATCH (f:FaceEmbedding {uuid:$id}) SET f.cluster=$cluster";
            let params = serde_json::json!({"id": id, "cluster": *cluster as i32});
            let payload =
                serde_json::json!({"statements":[{"statement":query,"parameters":params}]});
            let resp = self
                .client
                .post(self.neo4j_url.join("db/neo4j/tx/commit")?)
                .basic_auth(&self.neo_user, Some(&self.neo_pass))
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

fn cluster_vectors(vectors: &[Vec<f32>]) -> Result<Vec<usize>> {
    let data: Vec<Vec<f64>> = vectors
        .iter()
        .map(|v| v.iter().map(|&x| x as f64).collect())
        .collect();
    let m = DenseMatrix::from_2d_vec(&data)?;
    let kmeans = KMeans::<f64, usize, DenseMatrix<f64>, Vec<usize>>::fit(
        &m,
        KMeansParameters::default().with_k(8),
    )?;
    let labels = kmeans.predict(&m)?;
    Ok((0..labels.shape()).map(|i| *labels.get(i)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_vectors() {
        let data = vec![
            vec![1.0, 0.0],
            vec![0.9, 0.0],
            vec![0.0, 1.0],
            vec![0.0, 0.9],
        ];
        let labels = cluster_vectors(&data).unwrap();
        assert_eq!(labels.len(), 4);
    }
}
