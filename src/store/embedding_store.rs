use anyhow::Result;
use qdrant_client::{
    Payload, Qdrant,
    qdrant::{PointStruct, SearchPointsBuilder, UpsertPointsBuilder, point_id},
};
use serde_json::Value;
use uuid::Uuid;

/// Basic embedding store backed by a Qdrant collection.
///
/// This wraps [`Qdrant`] and exposes convenience helpers for
/// inserting vectors and querying nearest neighbours.
pub struct QdrantEmbeddingStore {
    /// Connected Qdrant client
    pub client: Qdrant,
    /// Target collection name
    pub collection: String,
}

impl QdrantEmbeddingStore {
    /// Create a new store using the given `client` and `collection` name.
    pub fn new(client: Qdrant, collection: impl Into<String>) -> Self {
        Self {
            client,
            collection: collection.into(),
        }
    }

    /// Insert a vector with associated `uuid` and `metadata`.
    pub async fn add_embedding(&self, uuid: Uuid, vector: Vec<f32>, metadata: Value) -> Result<()> {
        let payload = Payload::try_from(metadata)?;
        let point = PointStruct::new(uuid.to_string(), vector, payload);
        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]).wait(true))
            .await?;
        Ok(())
    }

    /// Search for vectors similar to the provided `query_embedding`.
    pub async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<Uuid>> {
        let res = self
            .client
            .search_points(SearchPointsBuilder::new(
                &self.collection,
                query_embedding,
                top_k as u64,
            ))
            .await?;

        let mut out = Vec::new();
        for scored in res.result {
            if let Some(id) = scored.id {
                if let Some(point_id::PointIdOptions::Uuid(u)) = id.point_id_options {
                    if let Ok(uuid) = Uuid::parse_str(&u) {
                        out.push(uuid);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Helper trait for retrieving similar memories based on text queries.
#[async_trait::async_trait(?Send)]
pub trait MemoryRetriever {
    /// Find memory identifiers similar to `text`.
    async fn find_similar(&self, text: &str, top_k: usize) -> Result<Vec<Uuid>>;
}

/// Very small utility used for examples and tests.
/// Converts text into a deterministic pseudo embedding.
pub fn simple_embed(text: &str) -> Vec<f32> {
    const DIM: usize = 8;
    let mut v = vec![0.0; DIM];
    for (i, b) in text.bytes().enumerate() {
        v[i % DIM] += b as f32;
    }
    v
}
