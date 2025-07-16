use super::{CanChat, LlmProfile};
use async_trait::async_trait;
use tokio_stream::{iter, Stream};

/// Simple echo chat implementation useful for tests.
#[derive(Default)]
pub struct EchoChat;

#[async_trait(?Send)]
impl CanChat for EchoChat {
    async fn chat_stream(
        &self,
        _profile: &LlmProfile,
        _system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        // In a real implementation this would call an API.
        let resp = format!("Echo: {}", user);
        Ok(Box::new(iter([resp.to_string()])))
    }
}
