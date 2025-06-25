use crate::memory::{Impression, Sensation, Urge};
use anyhow::Result;
use async_trait::async_trait;

/// Abstract interface for language model interactions used by cognitive wits.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Summarize a slice of [`Sensation`]s into a natural language description.
    async fn summarize(&self, input: &[Sensation]) -> Result<String>;

    /// Suggest potential [`Urge`]s based on the given [`Impression`].
    async fn suggest_urges(&self, impression: &Impression) -> Result<Vec<Urge>>;
}

/// Trivial implementation used for testing.
pub struct DummyLLM;

#[async_trait]
impl LLMClient for DummyLLM {
    async fn summarize(&self, input: &[Sensation]) -> Result<String> {
        Ok(format!("I'm seeing {} sensations.", input.len()))
    }

    async fn suggest_urges(&self, _impression: &Impression) -> Result<Vec<Urge>> {
        Ok(vec![])
    }
}
