use crate::llm::{prompt::PromptHelper, CanChat, CanEmbed, LlmProfile};
use crate::utils::{first_sentence, parse_json_or_string};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::StreamExt;
use tracing::{debug, info, trace};
#[cfg(feature = "qdrant")]
use uuid::Uuid;

/// Single memory entry linking a key sentence with a full body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Experience {
    /// One sentence summary of the body.
    pub how: String,
    /// The complete body content.
    pub what: Value,
    /// When the experience occurred.
    #[serde(with = "chrono::serde::ts_seconds")]
    pub when: DateTime<Utc>,
    /// Optional categorization tags.
    pub tags: Vec<String>,
}

#[cfg(feature = "neo4j")]
/// Fetch a compact contextual subgraph around a given [`Experience`] node.
///
/// This retrieves the experience itself, its immediate chronological
/// neighbors and simple causal and topical links. The query is optimized to
/// return at most seven related experiences.
///
/// # Example
///
/// ```
/// use async_trait::async_trait;
/// use psyche::memory::{context_subgraph, Experience, MemoryBackend};
///
/// struct Dummy;
///
/// #[async_trait(?Send)]
/// impl MemoryBackend for Dummy {
///     async fn store(&self, _: &Experience, _: &[f32]) -> anyhow::Result<String> { Ok("1".into()) }
///     async fn search(&self, _: &[f32], _: usize) -> anyhow::Result<Vec<Experience>> { Ok(vec![]) }
///     async fn get(&self, _: &str) -> anyhow::Result<Option<Experience>> { Ok(None) }
///     async fn cypher_query(&self, _: &str) -> anyhow::Result<Vec<Experience>> { Ok(vec![]) }
/// }
/// # tokio_test::block_on(async {
/// let backend = Dummy;
/// let _ = context_subgraph(&backend, "123").await;
/// # });
/// ```
pub async fn context_subgraph<B: MemoryBackend + Sync>(
    backend: &B,
    id: &str,
) -> anyhow::Result<Vec<Experience>> {
    debug!(id, "context_subgraph query");
    let query = format!(
        concat!(
            "MATCH (e:Experience {{id: \"{id}\"}})",
            "\nOPTIONAL MATCH (prev:Experience)-[:NEXT]->(e)",
            "\nOPTIONAL MATCH (e)-[:NEXT]->(next:Experience)",
            "\nOPTIONAL MATCH (e)-[:CAUSES]->(caused:Experience)",
            "\nOPTIONAL MATCH (causer:Experience)-[:CAUSES]->(e)",
            "\nOPTIONAL MATCH (e)-[:REFERS_TO]->(topic)<-[:REFERS_TO]-(related:Experience)",
            "\nWITH e, prev, next, caused, causer, collect(related)[0..2] AS topical",
            "\nUNWIND [e, prev, next, caused, causer] + topical AS node",
            "\nWITH DISTINCT node WHERE node IS NOT NULL",
            "\nRETURN node.how AS how, node.what AS what, node.when AS when, node.tags AS tags",
            "\nLIMIT 7"
        ),
        id = id
    );
    trace!(%query, "context_subgraph cypher");
    backend.cypher_query(&query).await
}

/// Result of persisting an experience.
#[derive(Debug)]
pub struct StoredExperience {
    /// Stored experience data.
    pub experience: Experience,
    /// Embedding vector used for vector search.
    pub vector: Vec<f32>,
}

/// Simple memory interface combining vector and graph storage.
#[async_trait(?Send)]
pub trait MemoryBackend {
    /// Store the given experience and embedding returning the backend id.
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<String>;

    /// Link a summary experience to the original it summarizes.
    async fn link_summary(&self, _summary_id: &str, _original_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Link a recalled memory to the query that triggered it.
    async fn link_called_to_mind(&self, _from: &str, _to: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Find similar experiences using cosine similarity or a vector database.
    async fn search(&self, vector: &[f32], top_k: usize) -> anyhow::Result<Vec<Experience>>;

    /// Retrieve a single experience by backend-defined identifier, if supported.
    async fn get(&self, id: &str) -> anyhow::Result<Option<Experience>>;

    /// Execute a custom Cypher query against the graph store if available.
    #[cfg(feature = "neo4j")]
    async fn cypher_query(&self, query: &str) -> anyhow::Result<Vec<Experience>>;
}

/// Captures experiences using a summarizer and embedder before persisting them
/// via a [`MemoryBackend`].
///
/// # Example
///
/// ```
/// use psyche::llm::{mock_chat::MockChat, mock_embed::MockEmbed, LlmCapability, LlmProfile, LlmRegistry};
/// use psyche::memory::{InMemoryBackend, Memorizer};
/// use psyche::llm::prompt::PromptHelper;
/// # tokio_test::block_on(async {
/// let profile = LlmProfile {
///     provider: "mock".into(),
///     model: "mock".into(),
///     capabilities: vec![LlmCapability::Chat, LlmCapability::Embedding],
/// };
/// let registry = LlmRegistry { chat: Box::new(MockChat::default()), embed: Box::new(MockEmbed::default()) };
/// let backend = InMemoryBackend::default();
/// let memorizer = Memorizer {
///     chat: Some(&*registry.chat),
///     embed: &*registry.embed,
///     profile: &profile,
///     backend: &backend,
///     prompter: PromptHelper::default(),
/// };
/// let exp = memorizer.memorize("the body", None, true, vec![]).await.unwrap();
/// assert_eq!(exp.experience.how, "mock response");
/// # });
/// ```
pub struct Memorizer<'a, B> {
    /// Optional chat model used for summary generation.
    pub chat: Option<&'a dyn CanChat>,
    /// Embedder for vector representations.
    pub embed: &'a dyn CanEmbed,
    /// LLM profile describing the models.
    pub profile: &'a LlmProfile,
    /// Backend that receives stored experiences.
    pub backend: B,
    /// Helper for augmenting prompts with the self header.
    pub prompter: PromptHelper,
}

impl<'a, B> Memorizer<'a, B>
where
    B: MemoryBackend + Sync,
{
    /// Persist a new experience. If `how` is `None` and `generate_summary` is
    /// `true`, the `chat` model will be used to create a one-sentence summary.
    pub async fn memorize(
        &self,
        what: &str,
        how: Option<&str>,
        generate_summary: bool,
        tags: Vec<String>,
    ) -> anyhow::Result<StoredExperience> {
        debug!(generate_summary, has_how = how.is_some(), tags = ?tags, "memorize called");
        let summary = if let Some(h) = how {
            first_sentence(h)
        } else if generate_summary {
            let chat = self
                .chat
                .ok_or_else(|| anyhow::anyhow!("no chat model configured"))?;
            let prompt = format!(
                "Summarize this as one emotionally descriptive sentence.\n\n{}",
                what
            );
            let system = self.prompter.system();
            trace!(target = "llm", system = %system, user = %prompt, "summary prompt");
            let mut stream = chat.chat_stream(self.profile, system, &prompt).await?;
            let mut out = String::new();
            while let Some(token) = stream.next().await {
                out.push_str(&token);
            }
            debug!(target = "llm", response = %out, "summary response");
            first_sentence(&out)
        } else {
            String::new()
        };

        let vector = self.embed.embed(self.profile, &summary).await?;
        let exp = Experience {
            how: summary,
            what: parse_json_or_string(what),
            when: Utc::now(),
            tags,
        };
        info!(tags = ?exp.tags, "storing experience");
        let _ = self.backend.store(&exp, &vector).await?;
        Ok(StoredExperience {
            experience: exp,
            vector,
        })
    }
}

/// In-memory backend used in tests and examples.
pub struct InMemoryBackend {
    /// Stored experiences for inspection.
    pub data: std::sync::Mutex<Vec<(Experience, Vec<f32>)>>,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b + 1e-8)
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        debug!("creating InMemoryBackend");
        Self {
            data: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait(?Send)]
impl MemoryBackend for InMemoryBackend {
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<String> {
        debug!(how = %exp.how, tags = ?exp.tags, "in-memory store");
        let mut data = self.data.lock().unwrap();
        data.push((exp.clone(), vector.to_vec()));
        let id = data.len() - 1;
        Ok(id.to_string())
    }

    async fn search(&self, vector: &[f32], top_k: usize) -> anyhow::Result<Vec<Experience>> {
        debug!(top_k, "in-memory search");
        let data = self.data.lock().unwrap();
        let mut scored: Vec<(f32, Experience)> = data
            .iter()
            .map(|(exp, v)| (cosine_similarity(vector, v), exp.clone()))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        Ok(scored.into_iter().take(top_k).map(|(_, e)| e).collect())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Experience>> {
        debug!(id, "in-memory get");
        if let Ok(index) = id.parse::<usize>() {
            let data = self.data.lock().unwrap();
            if let Some((exp, _)) = data.get(index) {
                return Ok(Some(exp.clone()));
            }
        }
        Ok(None)
    }

    async fn link_summary(&self, _summary_id: &str, _original_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "neo4j")]
    async fn cypher_query(&self, _query: &str) -> anyhow::Result<Vec<Experience>> {
        Ok(Vec::new())
    }
}

#[async_trait(?Send)]
impl MemoryBackend for &InMemoryBackend {
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<String> {
        debug!(how = %exp.how, "in-memory store ref");
        let mut data = self.data.lock().unwrap();
        data.push((exp.clone(), vector.to_vec()));
        let id = data.len() - 1;
        Ok(id.to_string())
    }

    async fn search(&self, vector: &[f32], top_k: usize) -> anyhow::Result<Vec<Experience>> {
        debug!(top_k, "in-memory search ref");
        (**self).search(vector, top_k).await
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Experience>> {
        debug!(id, "in-memory get ref");
        (**self).get(id).await
    }

    async fn link_summary(&self, _summary_id: &str, _original_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "neo4j")]
    async fn cypher_query(&self, query: &str) -> anyhow::Result<Vec<Experience>> {
        (**self).cypher_query(query).await
    }
}

#[cfg(feature = "qdrant")]
mod qdrant_store {
    use super::*;
    use qdrant_client::prelude::*;
    use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};

    #[async_trait(?Send)]
    pub trait CollectionMaker {
        async fn collection_exists(&self, name: &str) -> anyhow::Result<bool>;
        async fn create_memory_collection(&self, dim: u64) -> anyhow::Result<()>;
    }

    #[async_trait(?Send)]
    impl CollectionMaker for QdrantClient {
        async fn collection_exists(&self, name: &str) -> anyhow::Result<bool> {
            Ok(self.collection_exists(name).await?)
        }

        async fn create_memory_collection(&self, dim: u64) -> anyhow::Result<()> {
            let req = CreateCollectionBuilder::new("memory")
                .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine))
                .build();
            self.create_collection(&req).await?;
            Ok(())
        }
    }

    /// Ensure the `memory` collection exists creating it on first use.
    pub async fn ensure_collection(client: &impl CollectionMaker, dim: u64) -> anyhow::Result<()> {
        if !client.collection_exists("memory").await? {
            client.create_memory_collection(dim).await?;
        }
        Ok(())
    }

    /// Qdrant + Neo4j backend implementation.
    pub struct QdrantNeo4j {
        pub qdrant: QdrantClient,
        #[cfg(feature = "neo4j")]
        pub graph: neo4rs::Graph,
    }

    #[async_trait(?Send)]
    impl MemoryBackend for QdrantNeo4j {
        async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<String> {
            ensure_collection(&self.qdrant, vector.len() as u64).await?;
            let id = Uuid::new_v4().to_string();
            let points = vec![PointStruct::new(
                id.clone(),
                vector.to_vec(),
                std::collections::HashMap::<String, qdrant_client::qdrant::Value>::new(),
            )];
            self.qdrant
                .upsert_points_blocking("memory", None, points, None)
                .await?;

            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                self.graph
                    .run(
                        query(
                            "CREATE (:Experience {id: $id, how: $how, what: $what, when: $when, tags: $tags})",
                        )
                        .param("id", id.clone())
                        .param("how", exp.how.clone())
                        .param("what", exp.what.to_string())
                        .param("when", exp.when.to_rfc3339())
                        .param("tags", exp.tags.clone()),
                    )
                    .await?;
            }
            Ok(id)
        }

        async fn search(&self, vector: &[f32], top_k: usize) -> anyhow::Result<Vec<Experience>> {
            let request = SearchPoints {
                collection_name: "memory".into(),
                vector: vector.to_vec(),
                filter: None,
                limit: top_k as u64,
                with_payload: None,
                params: None,
                score_threshold: None,
                offset: None,
                vector_name: None,
                with_vectors: None,
                read_consistency: None,
                timeout: None,
                shard_key_selector: None,
                sparse_indices: None,
            };
            let search_result = self.qdrant.search_points(&request).await?;
            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                use qdrant_client::qdrant::point_id;
                let ids: Vec<String> = search_result
                    .result
                    .iter()
                    .filter_map(|pt| pt.id.as_ref())
                    .filter_map(|p| match p.point_id_options.as_ref()? {
                        point_id::PointIdOptions::Uuid(u) => Some(u.clone()),
                        point_id::PointIdOptions::Num(n) => Some(n.to_string()),
                    })
                    .collect();

                if ids.is_empty() {
                    return Ok(Vec::new());
                }

                let mut rows = self
                    .graph
                    .execute(query("MATCH (e:Experience) WHERE e.id IN $ids RETURN e.how AS how, e.what AS what, e.when AS when, e.tags AS tags")
                        .param("ids", ids))
                    .await?;
                let mut out = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let how: String = row.get("how")?;
                    let what: serde_json::Value = row.get("what")?;
                    let when: String = row.get("when")?;
                    let tags: Vec<String> = row.get("tags")?;
                    let when = DateTime::parse_from_rfc3339(&when)?.with_timezone(&Utc);
                    out.push(Experience {
                        how,
                        what,
                        when,
                        tags,
                    });
                }
                return Ok(out);
            }

            #[allow(unreachable_code)]
            Ok(Vec::new())
        }

        async fn get(&self, id: &str) -> anyhow::Result<Option<Experience>> {
            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                let mut rows = self
                    .graph
                    .execute(query("MATCH (e:Experience {id: $id}) RETURN e.how AS how, e.what AS what, e.when AS when, e.tags AS tags")
                        .param("id", id))
                    .await?;
                if let Ok(Some(row)) = rows.next().await {
                    let how: String = row.get("how")?;
                    let what: serde_json::Value = row.get("what")?;
                    let when: String = row.get("when")?;
                    let tags: Vec<String> = row.get("tags")?;
                    let when = DateTime::parse_from_rfc3339(&when)?.with_timezone(&Utc);
                    return Ok(Some(Experience {
                        how,
                        what,
                        when,
                        tags,
                    }));
                }
            }
            Ok(None)
        }

        #[cfg(feature = "neo4j")]
        async fn cypher_query(&self, query_str: &str) -> anyhow::Result<Vec<Experience>> {
            use neo4rs::query;
            let mut rows = self.graph.execute(query(query_str)).await?;
            let mut out = Vec::new();
            while let Ok(Some(row)) = rows.next().await {
                let how: String = row.get("how")?;
                let what: serde_json::Value = row.get("what")?;
                let when: String = row.get("when")?;
                let tags: Vec<String> = row.get("tags")?;
                let when = DateTime::parse_from_rfc3339(&when)?.with_timezone(&Utc);
                out.push(Experience {
                    how,
                    what,
                    when,
                    tags,
                });
            }
            Ok(out)
        }

        async fn link_summary(&self, summary_id: &str, original_id: &str) -> anyhow::Result<()> {
            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                self.graph
                    .run(
                        query("MATCH (s:Experience {id: $sid}), (o:Experience {id: $oid}) MERGE (s)-[:SUMMARIZES]->(o)")
                            .param("sid", summary_id)
                            .param("oid", original_id),
                    )
                    .await?;
            }
            Ok(())
        }

        async fn link_called_to_mind(&self, from: &str, to: &str) -> anyhow::Result<()> {
            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                self.graph
                    .run(
                        query("MATCH (q:Experience {id: $from}), (r:Experience {id: $to}) MERGE (q)-[:CALLED_TO_MIND]->(r)")
                            .param("from", from)
                            .param("to", to),
                    )
                    .await?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "qdrant")]
pub use qdrant_store::{ensure_collection, CollectionMaker, QdrantNeo4j};
