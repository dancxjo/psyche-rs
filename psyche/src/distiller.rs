use crate::models::{Instant, MemoryEntry, Sensation};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
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

/// Asynchronously transforms memory entries from one kind to another.
#[async_trait(?Send)]
pub trait Distiller {
    /// Distill the provided memory entries.
    ///
    /// ```
    /// use psyche::distiller::{Combobulator, Distiller};
    /// use psyche::models::MemoryEntry;
    /// use chrono::Utc;
    /// use serde_json::json;
    /// use uuid::Uuid;
    ///
    /// # tokio_test::block_on(async {
    /// let mut d = Combobulator;
    /// let input = vec![MemoryEntry {
    ///     id: Uuid::new_v4(),
    ///     kind: "sensation/chat".into(),
    ///     when: Utc::now(),
    ///     what: json!("I feel lonely"),
    ///     how: String::new(),
    /// }];
    /// let out = d.distill(input).await.unwrap();
    /// assert_eq!(out[0].how, "The interlocutor feels lonely");
    /// # });
    /// ```
    async fn distill(&mut self, input: Vec<MemoryEntry>) -> anyhow::Result<Vec<MemoryEntry>>;
}

/// Simple implementation that summarizes basic chat feelings.
pub struct Combobulator;

#[async_trait(?Send)]
impl Distiller for Combobulator {
    async fn distill(&mut self, input: Vec<MemoryEntry>) -> anyhow::Result<Vec<MemoryEntry>> {
        let mut output = Vec::new();
        for entry in input {
            if entry.kind == "sensation/chat" {
                if let Some(text) = entry.what.as_str() {
                    if let Some(feeling) = text.strip_prefix("I feel ") {
                        output.push(MemoryEntry {
                            id: Uuid::new_v4(),
                            kind: "instant".to_string(),
                            when: Utc::now(),
                            what: json!([entry.id]),
                            how: format!("The interlocutor feels {}", feeling),
                        });
                    }
                }
            }
        }
        Ok(output)
    }
}

/// Passes memory entries through unchanged. Useful for chaining distillers in
/// tests.
pub struct Passthrough;

#[async_trait(?Send)]
impl Distiller for Passthrough {
    async fn distill(&mut self, input: Vec<MemoryEntry>) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(input)
    }
}
