use super::{CanEmbed, LlmProfile};
use async_trait::async_trait;

/// Generates a fixed embedding useful for tests.
#[derive(Default)]
pub struct StaticEmbed;

#[async_trait(?Send)]
impl CanEmbed for StaticEmbed {
    async fn embed(&self, _profile: &LlmProfile, text: &str) -> anyhow::Result<Vec<f32>> {
        // Very naive embedding: length of text as a single value.
        Ok(vec![text.len() as f32])
    }
}
