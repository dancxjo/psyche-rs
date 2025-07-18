use super::{CanChat, LlmProfile};
use async_trait::async_trait;
use tokio_stream::{iter, Stream};
use tracing::{debug, trace};

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
        trace!(target: "llm", "MockChat prompt: system='{}' user='{}'", _system, _user);
        let resp = "mock response".to_string();
        debug!(target: "llm", response = %resp, "MockChat full response");
        Ok(Box::new(iter([resp])))
    }
}

/// Mock chat that returns its configured name as the response.
#[derive(Clone)]
pub struct NamedMockChat {
    /// The text returned for any prompt.
    pub name: String,
}

impl Default for NamedMockChat {
    fn default() -> Self {
        Self {
            name: "mock".into(),
        }
    }
}

#[async_trait(?Send)]
impl CanChat for NamedMockChat {
    async fn chat_stream(
        &self,
        _profile: &LlmProfile,
        _system: &str,
        _user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        Ok(Box::new(iter([self.name.clone()])))
    }
}
