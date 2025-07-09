use async_trait::async_trait;
use chrono::Utc;
use psyche_rs::{Impression, MemoryStore, StoredImpression, StoredSensation};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    CollectionExistsRequest, CreateCollectionBuilder, Distance, VectorParamsBuilder,
    collections_client::CollectionsClient,
};
use reqwest::Client;
use serde_json::json;
use tonic::transport::Channel;
use tracing::{debug, error, info};
use url::Url;

/// Persist an impression to the provided store.
///
/// Clones the sensation data to avoid ownership issues.
pub async fn persist_impression<T: serde::Serialize + Clone>(
    store: &(dyn MemoryStore + Send + Sync),
    imp: Impression<T>,
    kind: &str,
) -> anyhow::Result<()> {
    debug!("persisting impression");
    let mut sensation_ids = Vec::new();
    for s in imp.what {
        let data = serde_json::to_string(&s.what)?;
        if let Some(existing) = store.find_sensation(&s.kind, &data).await? {
            sensation_ids.push(existing.id);
        } else {
            let sid = uuid::Uuid::new_v4().to_string();
            sensation_ids.push(sid.clone());
            let stored = StoredSensation {
                id: sid,
                kind: s.kind.clone(),
                when: s.when.with_timezone(&Utc),
                data,
            };
            store.store_sensation(&stored).await.map_err(|e| {
                error!(?e, "store_sensation failed");
                e
            })?;
        }
    }
    let stored_imp = StoredImpression {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        when: Utc::now(),
        how: imp.how,
        sensation_ids,
        impression_ids: Vec::new(),
    };
    store.store_impression(&stored_imp).await.map_err(|e| {
        error!(?e, "store_impression failed");
        e
    })
}

/// Ensure the `impressions` collection exists in Qdrant.
///
/// Sends a `GET` request to check if the collection is present. If it returns
/// a `404` status code, a `PUT` request is issued to create the collection with
/// appropriate vector parameters.
pub async fn ensure_impressions_collection_exists(
    client: &Client,
    qdrant_base_url: &Url,
) -> anyhow::Result<()> {
    let url = qdrant_base_url.join("collections/impressions")?;
    let resp = client.get(url.clone()).send().await?;

    let (status, body) = {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        debug!(%status, %body, "qdrant check response");
        (status, body)
    };

    if status.is_success() {
        info!("impressions collection exists");
        return Ok(());
    }

    if status != reqwest::StatusCode::NOT_FOUND {
        // resp has already been consumed, use the previously extracted status and body
        error!(%status, %body, "failed to query collection");
        anyhow::bail!("qdrant check failed: {status}");
    }

    let body = json!({
        "vectors": {
            "size": 768,
            "distance": "Cosine"
        }
    });
    let resp = client.put(url).json(&body).send().await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    debug!(%status, %body, "qdrant create response");
    if status.is_success() {
        info!("impressions collection created");
        Ok(())
    } else {
        error!(%status, %body, "failed to create collection");
        anyhow::bail!("failed to create collection: {status}");
    }
}

/// Trait for managing the face embeddings collection in Qdrant.
#[async_trait]
pub trait FaceCollectionClient {
    /// Check whether the `face_embeddings` collection exists.
    async fn collection_exists(&self) -> anyhow::Result<bool>;

    /// Create the `face_embeddings` collection.
    async fn create_collection(&self) -> anyhow::Result<()>;
}

#[async_trait]
impl FaceCollectionClient for Qdrant {
    async fn collection_exists(&self) -> anyhow::Result<bool> {
        self.collection_exists("face_embeddings")
            .await
            .map_err(|e| anyhow::Error::new(e))
    }

    async fn create_collection(&self) -> anyhow::Result<()> {
        self.create_collection(
            CreateCollectionBuilder::new("face_embeddings")
                .vectors_config(VectorParamsBuilder::new(512, Distance::Cosine)),
        )
        .await
        .map(|_| ())
        .map_err(|e| anyhow::Error::new(e))
    }
}

#[async_trait]
impl FaceCollectionClient for Channel {
    async fn collection_exists(&self) -> anyhow::Result<bool> {
        let mut client = CollectionsClient::new(self.clone());
        let req = CollectionExistsRequest {
            collection_name: "face_embeddings".to_string(),
        };
        let res = client.collection_exists(req).await?;
        Ok(res.into_inner().result.map(|r| r.exists).unwrap_or(false))
    }

    async fn create_collection(&self) -> anyhow::Result<()> {
        let mut client = CollectionsClient::new(self.clone());
        let req = CreateCollectionBuilder::new("face_embeddings")
            .vectors_config(VectorParamsBuilder::new(512, Distance::Cosine))
            .build();
        client.create(req).await?;
        Ok(())
    }
}

/// Ensure the `face_embeddings` collection exists in Qdrant.
pub async fn ensure_face_embeddings_collection_exists(
    client: &(impl FaceCollectionClient + Sync),
) -> anyhow::Result<()> {
    if client.collection_exists().await? {
        info!("face_embeddings collection exists");
        return Ok(());
    }
    client.create_collection().await?;
    info!("face_embeddings collection created");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use httpmock::prelude::*;
    use psyche_rs::{InMemoryStore, Sensation};
    use url::Url;

    #[tokio::test]
    async fn stores_impression_and_sensations() {
        let store = InMemoryStore::new();
        let sensation = Sensation {
            kind: "test".into(),
            when: Local::now(),
            what: "hi".to_string(),
            source: None,
        };
        let imp = Impression {
            how: "example".into(),
            what: vec![sensation],
        };
        assert!(persist_impression(&store, imp, "Instant").await.is_ok());
    }

    #[tokio::test]
    async fn reuses_existing_sensations() {
        let store = InMemoryStore::new();
        let s = Sensation {
            kind: "test".into(),
            when: Local::now(),
            what: "foo".to_string(),
            source: None,
        };
        let imp1 = Impression {
            how: "one".into(),
            what: vec![s.clone()],
        };
        persist_impression(&store, imp1, "Instant").await.unwrap();

        let imp2 = Impression {
            how: "two".into(),
            what: vec![s],
        };
        persist_impression(&store, imp2, "Instant").await.unwrap();

        let recent = store.fetch_recent_impressions(2).await.unwrap();
        let sid = &recent[0].sensation_ids[0];
        assert!(recent.iter().all(|i| i.sensation_ids.contains(sid)));
    }

    #[tokio::test]
    async fn creates_collection_when_missing() {
        let server = MockServer::start_async().await;
        let _get = server
            .mock_async(|when, then| {
                when.method(GET).path("/collections/impressions");
                then.status(404);
            })
            .await;
        let put = server
            .mock_async(|when, then| {
                when.method(PUT).path("/collections/impressions");
                then.status(200);
            })
            .await;
        let client = Client::new();
        let url = Url::parse(&server.url("")).unwrap();
        assert!(
            ensure_impressions_collection_exists(&client, &url)
                .await
                .is_ok()
        );
        assert!(put.hits_async().await > 0);
    }

    #[tokio::test]
    async fn does_nothing_when_collection_exists() {
        let server = MockServer::start_async().await;
        let get = server
            .mock_async(|when, then| {
                when.method(GET).path("/collections/impressions");
                then.status(200);
            })
            .await;
        let client = Client::new();
        let url = Url::parse(&server.url("")).unwrap();
        assert!(
            ensure_impressions_collection_exists(&client, &url)
                .await
                .is_ok()
        );
        get.assert();
    }

    #[tokio::test]
    async fn returns_error_on_failed_creation() {
        let server = MockServer::start_async().await;
        let _get = server
            .mock_async(|when, then| {
                when.method(GET).path("/collections/impressions");
                then.status(404);
            })
            .await;
        let _put = server
            .mock_async(|when, then| {
                when.method(PUT).path("/collections/impressions");
                then.status(500);
            })
            .await;
        let client = Client::new();
        let url = Url::parse(&server.url("")).unwrap();
        assert!(
            ensure_impressions_collection_exists(&client, &url)
                .await
                .is_err()
        );
    }

    struct StubFaceClient {
        exists: bool,
        created: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl FaceCollectionClient for StubFaceClient {
        async fn collection_exists(&self) -> anyhow::Result<bool> {
            Ok(self.exists)
        }

        async fn create_collection(&self) -> anyhow::Result<()> {
            *self.created.lock().unwrap() = true;
            Ok(())
        }
    }

    #[tokio::test]
    async fn creates_face_collection_when_missing() {
        let client = StubFaceClient {
            exists: false,
            created: std::sync::Mutex::new(false),
        };
        ensure_face_embeddings_collection_exists(&client)
            .await
            .unwrap();
        assert!(*client.created.lock().unwrap());
    }

    #[tokio::test]
    async fn does_nothing_when_face_collection_exists() {
        let client = StubFaceClient {
            exists: true,
            created: std::sync::Mutex::new(false),
        };
        ensure_face_embeddings_collection_exists(&client)
            .await
            .unwrap();
        assert!(!*client.created.lock().unwrap());
    }
}
