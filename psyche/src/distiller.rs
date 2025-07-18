use crate::llm::{CanChat, LlmProfile};
use crate::models::{Instant, MemoryEntry, Sensation};
use crate::utils::{first_sentence, parse_json_or_string};
use chrono::Utc;
use serde_json::Value;
use tokio_stream::StreamExt;
use tracing::{debug, trace};
use uuid::Uuid;

/// Distills a raw `Sensation` into an `Instant` when possible.
///
/// Currently supports chat sensations of the form "I feel <emotion>".
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
    /// replaced with the joined input contents.
    pub prompt_template: String,
    /// Optional post-processing hook applied to the LLM response.
    /// Receives the input entries and LLM response.
    pub post_process: Option<fn(&[MemoryEntry], &str) -> anyhow::Result<Value>>,
}

/// General-purpose distiller powered by a language model.
pub struct Distiller {
    /// Configuration for this distiller.
    pub config: DistillerConfig,
    /// LLM used to generate summaries.
    pub llm: Box<dyn CanChat>,
    /// Profile for the bound LLM.
    pub profile: LlmProfile,
}

impl Distiller {
    /// Distill a group of memory entries into a single output memory entry.
    pub async fn distill(&mut self, input: Vec<MemoryEntry>) -> anyhow::Result<Vec<MemoryEntry>> {
        // Filter by input kind
        let entries: Vec<_> = input
            .into_iter()
            .filter(|e| {
                e.kind == self.config.input_kind
                    || e.kind.starts_with(&format!("{}/", self.config.input_kind))
            })
            .collect();

        if entries.is_empty() {
            return Ok(Vec::new());
        }

        // Join inputs into a single prompt string
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

        trace!(target = "llm", prompt = %prompt, "distiller prompt");
        let mut stream = self.llm.chat_stream(&self.profile, "", &prompt).await?;
        let mut resp = String::new();
        while let Some(token) = stream.next().await {
            resp.push_str(&token);
        }
        debug!(target = "llm", response = %resp, "distiller response");

        // Optional post-processing
        let value = if let Some(pp) = self.config.post_process {
            pp(&entries, &resp)?
        } else {
            parse_json_or_string(&resp)
        };

        Ok(vec![MemoryEntry {
            id: Uuid::new_v4(),
            kind: self.config.output_kind.clone(),
            when: Utc::now(),
            what: value,
            how: first_sentence(&resp),
        }])
    }
}

/// Simple post processor that stores the source entry ids as a JSON array.
pub fn link_sources(entries: &[MemoryEntry], _resp: &str) -> anyhow::Result<Value> {
    Ok(serde_json::json!(entries
        .iter()
        .map(|e| e.id)
        .collect::<Vec<_>>()))
}
