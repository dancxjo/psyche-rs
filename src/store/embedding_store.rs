use qdrant_client::{
    Payload, Qdrant,
    qdrant::{PointStruct, SearchPointsBuilder, UpsertPointsBuilder},
};
use serde_json::Value;
use uuid::Uuid;

/// Store embeddings using [Qdrant](https://qdrant.tech/).
///
/// ```no_run
/// # use qdrant_client::{Qdrant, QdrantBuilder};
/// # use psyche_rs::store::embedding_store::QdrantEmbeddingStore;
/// # use uuid::Uuid;
/// # async fn example(client: Qdrant) -> anyhow::Result<()> {
/// let store = QdrantEmbeddingStore::new(client, "memory");
/// store.add_embedding(Uuid::new_v4(), vec![0.0_f32; 3], serde_json::json!({"k":"v"})).await?;
/// let _ids = store.search_similar(vec![0.0_f32;3], 5).await?;
/// # Ok(())
/// # }
/// ```
pub struct QdrantEmbeddingStore {
    /// Connected Qdrant client
    pub client: Qdrant,
    /// Name of the collection to operate on
    pub collection: String,
}

impl QdrantEmbeddingStore {
    /// Construct a new store for the given collection.
    pub fn new(client: Qdrant, collection: impl Into<String>) -> Self {
        Self {
            client,
            collection: collection.into(),
        }
    }

    /// Add a vector and payload associated with the given UUID.
    pub async fn add_embedding(
        &self,
        uuid: Uuid,
        vector: Vec<f32>,
        metadata: Value,
    ) -> anyhow::Result<()> {
        let payload = Payload::try_from(metadata)?;
        let point = PointStruct::new(uuid.to_string(), vector, payload);
        let request = UpsertPointsBuilder::new(&self.collection, vec![point]).wait(true);
        self.client.upsert_points(request).await?;
        Ok(())
    }

    /// Search for vectors similar to `query_embedding` and return matching UUIDs.
    pub async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> anyhow::Result<Vec<Uuid>> {
        let request = SearchPointsBuilder::new(&self.collection, query_embedding, top_k as u64);
        let response = self.client.search_points(request).await?;
        let ids = response
            .result
            .into_iter()
            .filter_map(|sp| sp.id)
            .filter_map(|id| match id.point_id_options {
                Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => {
                    Uuid::parse_str(&u).ok()
                }
                _ => None,
            })
            .collect();
        Ok(ids)
    }
}

#[async_trait::async_trait]
impl crate::memory::MemoryRetriever for QdrantEmbeddingStore {
    async fn find_similar(&self, text: &str, top_k: usize) -> anyhow::Result<Vec<Uuid>> {
        // Very naive embedding: convert bytes directly to floats.
        let embedding = text.bytes().map(|b| b as f32).collect::<Vec<_>>();
        self.search_similar(embedding, top_k).await
    }
}
