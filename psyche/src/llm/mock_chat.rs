use super::{CanChat, LlmProfile};
use async_trait::async_trait;
use tokio_stream::{iter, Stream};

/// Mock chat client returning a fixed response.
#[derive(Default)]
pub struct MockChat;

#[async_trait(?Send)]
impl CanChat for MockChat {
    async fn chat_stream(
        &self,
        _profile: &LlmProfile,
        _system: &str,
        _user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        Ok(Box::new(iter(["mock response".to_string()])))
    }
}
