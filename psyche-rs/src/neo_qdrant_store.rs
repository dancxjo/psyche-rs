use crate::llm_client::LLMClient;
use crate::memory_store::{MemoryStore, StoredImpression, StoredSensation};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{from_str, json};
use std::collections::HashMap;
use tracing::{debug, error};

/// Memory store backed by Neo4j for graph storage and Qdrant for vector search.
///
/// This implementation uses the transactional HTTP APIs of both services.
pub struct NeoQdrantMemoryStore {
    client: Client,
    neo4j_url: String,
    neo_user: String,
    neo_pass: String,
    qdrant_url: String,
    llm: std::sync::Arc<dyn LLMClient>,
}

impl NeoQdrantMemoryStore {
    pub fn new(
        neo4j_url: impl Into<String>,
        neo_user: impl Into<String>,
        neo_pass: impl Into<String>,
        qdrant_url: impl Into<String>,
        llm: std::sync::Arc<dyn LLMClient>,
    ) -> Self {
        Self {
            client: Client::new(),
            neo4j_url: neo4j_url.into(),
            neo_user: neo_user.into(),
            neo_pass: neo_pass.into(),
            qdrant_url: qdrant_url.into(),
            llm,
        }
    }

    /// Access the underlying HTTP client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Base URL of the Neo4j server.
    pub fn neo4j_url(&self) -> &str {
        &self.neo4j_url
    }

    /// Neo4j username.
    pub fn neo_user(&self) -> &str {
        &self.neo_user
    }

    /// Neo4j password.
    pub fn neo_pass(&self) -> &str {
        &self.neo_pass
    }

    /// Sanitize a string so it can be safely used as a Neo4j label.
    ///
    /// This replaces any character that is not alphanumeric with an underscore
    /// and ensures the label starts with an alphabetic character. The returned
    /// label is safe to embed directly in a Cypher query.
    fn sanitize_label(label: &str) -> String {
        let mut out: String = label
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        if out
            .chars()
            .next()
            .map(|c| !c.is_ascii_alphabetic())
            .unwrap_or(true)
        {
            out.insert(0, 'L');
        }
        out
    }

    async fn post_neo_async(&self, query: &str, params: serde_json::Value) -> anyhow::Result<()> {
        debug!(?query, "posting cypher");
        let payload = json!({"statements":[{"statement": query, "parameters": params}]});
        let resp = self
            .client
            .post(format!("{}/db/neo4j/tx/commit", self.neo4j_url))
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .context("neo4j request failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "neo4j error");
            Err(anyhow!("neo4j error: {}", body))
        }
    }
}

#[async_trait]
impl MemoryStore for NeoQdrantMemoryStore {
    async fn store_sensation(&self, sensation: &StoredSensation) -> anyhow::Result<()> {
        debug!(id = %sensation.id, "store sensation");
        let label = Self::sanitize_label(&sensation.kind);
        let query = format!(
            "MERGE (s:Sensation:`{}` {{uuid: $id}}) SET s.kind=$kind, s.when=datetime($when), s.data=$data",
            label
        );
        let params = json!({
            "id": sensation.id,
            "kind": sensation.kind,
            "when": sensation.when.to_rfc3339(),
            "data": sensation.data,
        });
        self.post_neo_async(&query, params).await
    }

    async fn find_sensation(
        &self,
        kind: &str,
        data: &str,
    ) -> anyhow::Result<Option<StoredSensation>> {
        let query = "MATCH (s:Sensation {kind:$kind, data:$data}) RETURN s LIMIT 1";
        let params = json!({"kind": kind, "data": data});
        let payload = json!({"statements":[{"statement": query, "parameters": params}]});
        let resp = self
            .client
            .post(format!("{}/db/neo4j/tx/commit", self.neo4j_url))
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .context("neo4j find sensation failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "neo4j error");
            return Err(anyhow!("neo4j error: {}", body));
        }
        #[derive(Deserialize)]
        struct R {
            results: Vec<R1>,
        }
        #[derive(Deserialize)]
        struct R1 {
            data: Vec<R2>,
        }
        #[derive(Deserialize)]
        struct R2 {
            row: (StoredSensation,),
        }
        let res: R = resp.json().await.context("parse neo4j find sensation")?;
        let sens = res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .next()
            .map(|r| r.row.0);
        Ok(sens)
    }

    async fn store_impression(&self, impression: &StoredImpression) -> anyhow::Result<()> {
        debug!(id = %impression.id, "store impression");
        let label = Self::sanitize_label(&impression.kind);
        let query = format!(
            r#"MERGE (i:Impression:`{label}` {{uuid:$id}})
SET i.kind=$kind, i.when=datetime($when), i.how=$how, i.summary_of=$imps
WITH i
UNWIND $sids AS sid
MATCH (s:Sensation {{uuid:sid}})
MERGE (i)-[:HAS_SENSATION]->(s)"#
        );
        let params = json!({
            "id": impression.id,
            "kind": impression.kind,
            "when": impression.when.to_rfc3339(),
            "how": impression.how,
            "sids": impression.sensation_ids,
            "imps": impression.impression_ids,
        });
        self.post_neo_async(&query, params).await?;

        let vector = self
            .llm
            .embed(&impression.how)
            .await
            .map_err(|e| anyhow!(e))?;
        let qbody = json!({
            "points": [{
                "id": impression.id,
                "vector": vector,
                "payload": {"how": impression.how}
            }]
        });
        let url = format!("{}/collections/impressions/points", self.qdrant_url);
        debug!(url = %url, "storing embedding to qdrant");
        let resp = self
            .client
            .put(url)
            .json(&qbody)
            .send()
            .await
            .context("qdrant insert failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "qdrant error");
            Err(anyhow!("qdrant error: {}", body))
        }
    }

    async fn store_summary_impression(
        &self,
        summary: &StoredImpression,
        linked_ids: &[String],
    ) -> anyhow::Result<()> {
        debug!(id = %summary.id, ?linked_ids, "store summary impression");
        let label = Self::sanitize_label(&summary.kind);
        let query = format!(
            r#"MERGE (i:Impression:`{label}` {{uuid:$id}})
SET i.kind=$kind, i.when=datetime($when), i.how=$how, i.summary_of=$imps
WITH i
UNWIND $imps AS iid
MATCH (o:Impression {{uuid:iid}})
MERGE (i)-[:SUMMARIZES]->(o)
WITH i
UNWIND $sids AS sid
MATCH (s:Sensation {{uuid:sid}})
MERGE (i)-[:HAS_SENSATION]->(s)"#
        );
        let params = json!({
            "id": summary.id,
            "kind": summary.kind,
            "when": summary.when.to_rfc3339(),
            "how": summary.how,
            "sids": summary.sensation_ids,
            "imps": linked_ids,
        });
        self.post_neo_async(&query, params).await?;

        let vector = self.llm.embed(&summary.how).await.map_err(|e| anyhow!(e))?;
        let qbody = json!({
            "points": [{
                "id": summary.id,
                "vector": vector,
                "payload": {"how": summary.how}
            }]
        });
        let url = format!("{}/collections/impressions/points", self.qdrant_url);
        debug!(url = %url, "storing embedding to qdrant");
        let resp = self
            .client
            .put(url)
            .json(&qbody)
            .send()
            .await
            .context("qdrant insert failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "qdrant error");
            Err(anyhow!("qdrant error: {}", body))
        }
    }

    async fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        debug!(impression_id, stage, "add lifecycle stage");
        let query = r#"
MATCH (i:Impression {uuid:$id})
MERGE (l:Lifecycle {uuid: $id || ':' || $stage})
SET l.stage=$stage, l.detail=$detail
MERGE (i)-[:HAS_STAGE]->(l)
"#;
        let params = json!({
            "id": impression_id,
            "stage": stage,
            "detail": detail,
        });
        self.post_neo_async(query, params).await
    }

    async fn retrieve_related_impressions(
        &self,
        query_how: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        debug!(?query_how, top_k, "retrieve related impressions");
        let vector = self.llm.embed(query_how).await.map_err(|e| anyhow!(e))?;
        let qbody = json!({"vector": vector, "limit": top_k});
        let url = format!("{}/collections/impressions/points/search", self.qdrant_url);
        let resp = self
            .client
            .post(url)
            .json(&qbody)
            .send()
            .await
            .context("qdrant search failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "qdrant search error");
            return Err(anyhow!("qdrant search error: {}", body));
        }
        #[derive(Deserialize)]
        struct SearchRes {
            result: Vec<SearchItem>,
        }
        #[derive(Deserialize)]
        struct SearchItem {
            id: String,
        }
        let ids: SearchRes = resp.json().await.context("parse search result")?;
        if ids.result.is_empty() {
            return Ok(Vec::new());
        }
        let ids_vec: Vec<String> = ids.result.into_iter().map(|s| s.id).collect();
        let query = "MATCH (i:Impression) WHERE i.uuid IN $ids RETURN i";
        let params = json!({"ids": ids_vec});
        let payload = json!({"statements":[{"statement":query, "parameters":params}]});
        let resp = self
            .client
            .post(format!("{}/db/neo4j/tx/commit", self.neo4j_url))
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .context("neo4j retrieve failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "neo4j error");
            return Err(anyhow!("neo4j error: {}", body));
        }
        #[derive(Deserialize)]
        struct NeoRes {
            results: Vec<NeoResult>,
        }
        #[derive(Deserialize)]
        struct NeoResult {
            data: Vec<NeoRow>,
        }
        #[derive(Deserialize)]
        struct NeoRow {
            row: (StoredImpression,),
        }
        let res: NeoRes = resp.json().await.context("parse neo4j result")?;
        let imps = res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .map(|r| r.row.0)
            .collect();
        Ok(imps)
    }

    async fn fetch_recent_impressions(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        debug!(limit, "fetch recent impressions");
        let query = "MATCH (i:Impression) RETURN i ORDER BY i.when DESC LIMIT $limit";
        let params = json!({"limit": limit});
        let payload = json!({"statements":[{"statement":query, "parameters":params}]});
        let resp = self
            .client
            .post(format!("{}/db/neo4j/tx/commit", self.neo4j_url))
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .context("neo4j recent failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "neo4j error");
            return Err(anyhow!("neo4j error: {}", body));
        }
        #[derive(Deserialize)]
        struct Res {
            results: Vec<R1>,
        }
        #[derive(Deserialize)]
        struct R1 {
            data: Vec<R2>,
        }
        #[derive(Deserialize)]
        struct R2 {
            row: (StoredImpression,),
        }
        let status = resp.status();
        let body = resp.text().await.context("neo4j recent body")?;
        let res: Res = from_str(&body)
            .with_context(|| format!("parse neo4j recent (status {status}): {body}"))?;
        let out = res
            .results
            .into_iter()
            .flat_map(|r| r.data)
            .map(|r| r.row.0)
            .collect();
        Ok(out)
    }

    async fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        HashMap<String, String>,
    )> {
        debug!(impression_id, "load full impression");
        let query = r#"
MATCH (i:Impression {uuid:$id})
OPTIONAL MATCH (i)-[:HAS_SENSATION]->(s:Sensation)
OPTIONAL MATCH (i)-[:HAS_STAGE]->(l:Lifecycle)
RETURN i, collect(DISTINCT s) AS sens, collect(DISTINCT l) AS stages
"#;
        let params = json!({"id": impression_id});
        let payload = json!({"statements":[{"statement":query, "parameters":params}]});
        let resp = self
            .client
            .post(format!("{}/db/neo4j/tx/commit", self.neo4j_url))
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .context("neo4j load full failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, %body, "neo4j error");
            return Err(anyhow!("neo4j error: {}", body));
        }
        #[derive(Deserialize)]
        struct R {
            results: Vec<R2>,
        }
        #[derive(Deserialize)]
        struct R2 {
            data: Vec<R3>,
        }
        #[derive(Deserialize)]
        struct R3 {
            row: (StoredImpression, Vec<StoredSensation>, Vec<StageNode>),
        }
        #[derive(Deserialize)]
        struct StageNode {
            _uuid: String,
            stage: String,
            detail: String,
        }
        let res: R = resp.json().await.context("parse neo4j full result")?;
        let row = res
            .results
            .into_iter()
            .next()
            .and_then(|r| r.data.into_iter().next())
            .ok_or_else(|| anyhow!("no data"))?;
        let (imp, sens, stages_raw) = row.row;
        let mut stages = HashMap::new();
        for st in stages_raw {
            stages.insert(st.stage, st.detail);
        }
        Ok((imp, sens, stages))
    }

    async fn delete_impression(&self, impression_id: &str) -> anyhow::Result<()> {
        let query = "MATCH (i:Impression {uuid:$id}) DETACH DELETE i";
        let params = json!({"id": impression_id});
        self.post_neo_async(query, params).await?;

        let url = format!("{}/collections/impressions/points/delete", self.qdrant_url);
        let body = json!({"points": [impression_id]});
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .context("qdrant delete failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!(status = %status, %text, "qdrant error");
            Err(anyhow!("qdrant error: {}", text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::StaticLLM;
    use chrono::Utc;
    use httpmock::prelude::*;
    use serde_json::json;

    #[tokio::test]
    async fn store_impression_hits_backends() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_sens_mock = neo.mock(|when, then| {
            when.method(POST).body_contains("Sensation:`test`");
            then.status(200);
        });
        let neo_imp_mock = neo.mock(|when, then| {
            when.method(POST).body_contains(":`Situation`");
            then.status(200);
        });
        let q_mock = qdrant.mock(|when, then| {
            when.method(PUT);
            then.status(200);
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "user", "pass", qdrant.url(""), llm);

        let sens = StoredSensation {
            id: "s1".into(),
            kind: "test".into(),
            when: Utc::now(),
            data: "{}".into(),
        };
        store.store_sensation(&sens).await.unwrap();

        let imp = StoredImpression {
            id: "i1".into(),
            kind: "Situation".into(),
            when: Utc::now(),
            how: "hi".into(),
            sensation_ids: vec!["s1".into()],
            impression_ids: Vec::new(),
        };
        store.store_impression(&imp).await.unwrap();

        assert_eq!(neo_sens_mock.hits(), 1);
        assert_eq!(neo_imp_mock.hits(), 1);
        assert_eq!(q_mock.hits(), 1);
    }

    #[tokio::test]
    async fn store_sensation_uses_kind_label() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_mock = neo.mock(|when, then| {
            when.method(POST).body_contains(":`test`");
            then.status(200);
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);

        let sens = StoredSensation {
            id: "s1".into(),
            kind: "test".into(),
            when: Utc::now(),
            data: "{}".into(),
        };
        store.store_sensation(&sens).await.unwrap();

        assert_eq!(neo_mock.hits(), 1);
    }

    #[tokio::test]
    async fn store_summary_impression_creates_links() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_mock = neo.mock(|when, then| {
            when.method(POST)
                .path("/db/neo4j/tx/commit")
                .body_contains("SUMMARIZES")
                .body_contains(":`Summary`");
            then.status(200);
        });
        let q_mock = qdrant.mock(|when, then| {
            when.method(PUT);
            then.status(200);
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);

        let summary = StoredImpression {
            id: "s".into(),
            kind: "Summary".into(),
            when: Utc::now(),
            how: "sum".into(),
            sensation_ids: Vec::new(),
            impression_ids: vec!["i1".into(), "i2".into()],
        };

        store
            .store_summary_impression(&summary, &summary.impression_ids)
            .await
            .unwrap();

        assert_eq!(neo_mock.hits(), 1);
        assert_eq!(q_mock.hits(), 1);
    }

    #[tokio::test]
    async fn fetch_recent_queries_neo4j() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({
                "results": [{"data": []}]
            }));
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);
        let _ = store.fetch_recent_impressions(5).await;
        assert_eq!(neo_mock.hits(), 1);
    }

    #[tokio::test]
    async fn retrieve_related_hits_qdrant() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let q_mock = qdrant.mock(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({"result": []}));
        });

        let neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({
                "results": [{"data": []}]
            }));
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);
        let _ = store.retrieve_related_impressions("hi", 3).await;
        assert_eq!(q_mock.hits(), 1);
        assert_eq!(neo_mock.hits(), 0);
    }

    #[tokio::test]
    async fn store_impression_error_propagates() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let _neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200);
        });
        let q_mock = qdrant.mock(|when, then| {
            when.method(PUT);
            then.status(500);
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);
        let sens = StoredSensation {
            id: "s".into(),
            kind: "t".into(),
            when: Utc::now(),
            data: "{}".into(),
        };
        store.store_sensation(&sens).await.unwrap();

        let imp = StoredImpression {
            id: "i".into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "hi".into(),
            sensation_ids: vec!["s".into()],
            impression_ids: Vec::new(),
        };
        assert!(store.store_impression(&imp).await.is_err());
        assert_eq!(q_mock.hits(), 1);
    }

    #[tokio::test]
    async fn fetch_recent_parse_error_body_included() {
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200).body("not json");
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);
        let err = store.fetch_recent_impressions(5).await.unwrap_err();
        assert!(err.to_string().contains("not json"));
        assert_eq!(neo_mock.hits(), 1);
    }

    #[tokio::test]
    async fn persist_impression_reuses_sensations_neo() {
        use crate::psyche::persist_impression;
        use crate::{Impression, Sensation};
        let neo = MockServer::start();
        let qdrant = MockServer::start();

        let neo_mock = neo.mock(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({
                "results": [{
                    "data": [{
                        "row": [{
                            "id": "s1",
                            "kind": "t",
                            "when": Utc::now().to_rfc3339(),
                            "data": "\"foo\""
                        }]
                    }]
                }]
            }));
        });
        let q_mock = qdrant.mock(|when, then| {
            when.method(PUT);
            then.status(200);
        });

        let llm = std::sync::Arc::new(StaticLLM::new(""));
        let store = NeoQdrantMemoryStore::new(neo.url(""), "u", "p", qdrant.url(""), llm);

        // seed existing sensation
        let sens = StoredSensation {
            id: "s1".into(),
            kind: "t".into(),
            when: Utc::now(),
            data: "\"foo\"".into(),
        };
        store.store_sensation(&sens).await.unwrap();

        let s = Sensation::<String> {
            kind: "t".into(),
            when: chrono::Local::now(),
            what: "foo".into(),
            source: None,
        };
        let imp = Impression::new(vec![s], "hi").unwrap();
        persist_impression(&store, imp, "Instant").await.unwrap();

        assert_eq!(neo_mock.hits(), 3);
        assert_eq!(q_mock.hits(), 1);
    }
}
