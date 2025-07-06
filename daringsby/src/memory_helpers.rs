use chrono::Utc;
use psyche_rs::{Impression, MemoryStore, StoredImpression, StoredSensation};
use reqwest::Client;
use serde_json::json;
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
        let sid = uuid::Uuid::new_v4().to_string();
        sensation_ids.push(sid.clone());
        let stored = StoredSensation {
            id: sid,
            kind: s.kind.clone(),
            when: s.when.with_timezone(&Utc),
            data: serde_json::to_string(&s.what)?,
        };
        store.store_sensation(&stored).await.map_err(|e| {
            error!(?e, "store_sensation failed");
            e
        })?;
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

    #[cfg(feature = "debug_memory")]
    {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        debug!(%status, %body, "qdrant check response");
    }

    // resp is moved above in debug_memory, so we need to get status and body before consuming resp
    #[cfg(not(feature = "debug_memory"))]
    let status = resp.status();
    #[cfg(not(feature = "debug_memory"))]
    let body = resp.text().await.unwrap_or_default();

    #[cfg(feature = "debug_memory")]
    let (status, body) = {
        // These are already handled in the debug block above, so just set dummy values
        (reqwest::StatusCode::OK, String::new())
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

    #[cfg(feature = "debug_memory")]
    {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        debug!(%status, %body, "qdrant create response");
        // resp is consumed here, so we need to return after debug or refetch
        return if status.is_success() {
            info!("impressions collection created");
            Ok(())
        } else {
            error!(%status, %body, "failed to create collection");
            anyhow::bail!("failed to create collection: {status}");
        };
    }

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.is_success() {
        info!("impressions collection created");
        Ok(())
    } else {
        error!(%status, %body, "failed to create collection");
        anyhow::bail!("failed to create collection: {status}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use httpmock::prelude::*;
    use psyche_rs::{InMemoryStore, Sensation};
    use serde_json::json;
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
                when.method(PUT)
                    .path("/collections/impressions")
                    .json_body(json!({"vectors": {"size": 768, "distance": "Cosine"}}));
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
        put.assert();
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
}
