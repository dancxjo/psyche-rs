use crate::llm::{CanChat, LlmCapability, LlmProfile};
use crate::models::{Instant, MemoryEntry, Sensation};
use chrono::Utc;
use serde_json::Value;
use tokio_stream::StreamExt;
use tracing::{debug, trace};
use uuid::Uuid;

/// Distills a raw `Sensation` into an `Instant` when possible.
///
/// Currently supports chat sensations of the form "I feel <emotion>".
///
/// # Examples
///
/// ```
/// use psyche::distiller::distill;
/// use psyche::models::Sensation;
///
/// let s = Sensation {
///     id: "1".into(),
///     path: "/chat".into(),
///     text: "I feel lonely".into(),
/// };
/// let instant = distill(&s).unwrap();
/// assert_eq!(instant.how, "The interlocutor feels lonely");
/// ```
pub fn distill(sensation: &Sensation) -> Option<Instant> {
    if sensation.path == "/chat" && sensation.text.starts_with("I feel ") {
        let feeling = sensation.text.trim_start_matches("I feel ");
        Some(Instant {
            kind: "instant".to_string(),
            how: format!("The interlocutor feels {}", feeling),
            what: vec![sensation.id.clone()],
        })
    } else {
        None
    }
}

/// Configuration for a [`Distiller`].
#[derive(Clone, Debug)]
pub struct DistillerConfig {
    /// Human readable name for this distiller.
    pub name: String,
    /// Input memory kind this distiller consumes.
    pub input_kind: String,
    /// Kind of memory entry produced.
    pub output_kind: String,
    /// Prompt template used when chatting with the LLM. The literal "{input}" is
    /// replaced with the incoming text.
    pub prompt_template: String,
    /// Optional post-processing hook applied to the LLM response. The slice of
    /// input entries consumed by this distillation is provided to allow linking
    /// to the originals.
    pub post_process: Option<fn(&[MemoryEntry], &str) -> anyhow::Result<Value>>,
}

/// General-purpose distiller powered by a language model.
pub struct Distiller {
    /// Configuration for this distiller.
    pub config: DistillerConfig,
    /// LLM used to generate summaries.
    pub llm: Box<dyn CanChat>,
}

impl Distiller {
    /// Distill the provided entries into the configured output kind.
    ///
    /// ```
    /// use psyche::distiller::{Distiller, DistillerConfig};
    /// use psyche::llm::mock_chat::MockChat;
    /// use psyche::models::MemoryEntry;
    /// use chrono::Utc;
    /// use serde_json::json;
    /// use uuid::Uuid;
    ///
    /// # tokio_test::block_on(async {
    /// let cfg = DistillerConfig {
    ///     name: "echo".into(),
    ///     input_kind: "sensation/chat".into(),
    ///     output_kind: "instant".into(),
    ///     prompt_template: "{input}".into(),
    ///     post_process: None,
    /// };
    /// let mut d = Distiller { config: cfg, llm: Box::new(MockChat::default()) };
    /// let input = vec![MemoryEntry {
    ///     id: Uuid::new_v4(),
    ///     kind: "sensation/chat".into(),
    ///     when: Utc::now(),
    ///     what: json!("hello"),
    ///     how: String::new(),
    /// }];
    /// let out = d.distill(input).await.unwrap();
    /// assert_eq!(out[0].how, "mock response");
    /// # });
    /// ```
    pub async fn distill(&mut self, input: Vec<MemoryEntry>) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries: Vec<_> = input
            .into_iter()
            .filter(|e| e.kind == self.config.input_kind)
            .collect();

        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let joined = entries
            .iter()
            .map(|e| {
                if !e.how.is_empty() {
                    e.how.clone()
                } else {
                    e.what
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| e.what.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = self.config.prompt_template.replace("{input}", &joined);
        let profile = LlmProfile {
            provider: "local".into(),
            model: "mock".into(),
            capabilities: vec![LlmCapability::Chat],
        };

        trace!(target = "llm", prompt = %prompt, "distiller prompt");
        let mut stream = self.llm.chat_stream(&profile, "", &prompt).await?;
        let mut resp = String::new();
        while let Some(token) = stream.next().await {
            resp.push_str(&token);
        }
        debug!(target = "llm", response = %resp, "distiller response");

        let value = if let Some(pp) = self.config.post_process {
            pp(&entries, &resp)?
        } else {
            Value::String(resp.clone())
        };

        let out = MemoryEntry {
            id: Uuid::new_v4(),
            kind: self.config.output_kind.clone(),
            when: Utc::now(),
            what: value,
            how: resp,
        };

        Ok(vec![out])
    }
}

/// Simple post processor that stores the source entry ids as a JSON array.
pub fn link_sources(entries: &[MemoryEntry], _resp: &str) -> anyhow::Result<Value> {
    Ok(serde_json::json!(entries
        .iter()
        .map(|e| e.id)
        .collect::<Vec<_>>()))
}
