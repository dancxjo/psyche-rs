use crate::llm::{CanChat, CanEmbed, LlmProfile};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::StreamExt;
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
    /// Store the given experience and embedding.
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<()>;
}

/// Captures experiences using a summarizer and embedder before persisting them
/// via a [`MemoryBackend`].
///
/// # Example
///
/// ```
/// use psyche::llm::{mock_chat::MockChat, mock_embed::MockEmbed, LlmCapability, LlmProfile, LlmRegistry};
/// use psyche::memory::{InMemoryBackend, Memorizer};
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
        let summary = if let Some(h) = how {
            h.to_string()
        } else if generate_summary {
            let chat = self
                .chat
                .ok_or_else(|| anyhow::anyhow!("no chat model configured"))?;
            let prompt = format!(
                "Summarize this as one emotionally descriptive sentence.\n\n{}",
                what
            );
            let mut stream = chat.chat_stream(self.profile, "", &prompt).await?;
            let mut out = String::new();
            while let Some(token) = stream.next().await {
                out.push_str(&token);
            }
            out
        } else {
            String::new()
        };

        let vector = self.embed.embed(self.profile, &summary).await?;
        let exp = Experience {
            how: summary,
            what: Value::String(what.to_string()),
            when: Utc::now(),
            tags,
        };
        self.backend.store(&exp, &vector).await?;
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

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self {
            data: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait(?Send)]
impl MemoryBackend for InMemoryBackend {
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .push((exp.clone(), vector.to_vec()));
        Ok(())
    }
}

#[async_trait(?Send)]
impl MemoryBackend for &InMemoryBackend {
    async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .push((exp.clone(), vector.to_vec()));
        Ok(())
    }
}

#[cfg(feature = "qdrant")]
mod qdrant_store {
    use super::*;
    use qdrant_client::prelude::*;

    /// Qdrant + Neo4j backend implementation.
    pub struct QdrantNeo4j {
        pub qdrant: QdrantClient,
        #[cfg(feature = "neo4j")]
        pub graph: neo4rs::Graph,
    }

    #[async_trait(?Send)]
    impl MemoryBackend for QdrantNeo4j {
        async fn store(&self, exp: &Experience, vector: &[f32]) -> anyhow::Result<()> {
            let id = Uuid::new_v4().to_string();
            let points = vec![PointStruct::new(id.clone(), vector.to_vec())];
            self.qdrant
                .upload_points_blocking("memory", None, points, None)
                .await?;

            #[cfg(feature = "neo4j")]
            {
                use neo4rs::query;
                self.graph
                    .run(
                        query(
                            "CREATE (:Experience {id: $id, how: $how, what: $what, when: $when, tags: $tags})",
                        )
                        .param("id", id)
                        .param("how", &exp.how)
                        .param("what", &exp.what)
                        .param("when", exp.when.to_rfc3339())
                        .param("tags", &exp.tags),
                    )
                    .await?;
            }
            Ok(())
        }
    }
}
