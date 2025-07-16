use super::{CanEmbed, LlmProfile};
use async_trait::async_trait;

/// Mock embedder returning zeros.
#[derive(Default)]
pub struct MockEmbed;

#[async_trait(?Send)]
impl CanEmbed for MockEmbed {
    async fn embed(&self, _profile: &LlmProfile, _text: &str) -> anyhow::Result<Vec<f32>> {
        Ok(vec![0.0, 0.0, 0.0])
    }
}
