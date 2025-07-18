use super::{CanChat, LlmProfile};
use async_trait::async_trait;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_stream::Stream;

/// Wrapper that limits concurrent access to an inner [`CanChat`].
pub struct LimitedChat {
    inner: Arc<dyn CanChat>,
    semaphore: Arc<Semaphore>,
}

impl LimitedChat {
    pub fn new(inner: Arc<dyn CanChat>, semaphore: Arc<Semaphore>) -> Self {
        Self { inner, semaphore }
    }
}

struct PermitStream<S> {
    stream: S,
    _permit: OwnedSemaphorePermit,
}

impl<S: Stream + Unpin> Stream for PermitStream<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

#[async_trait(?Send)]
impl CanChat for LimitedChat {
    async fn chat_stream(
        &self,
        profile: &LlmProfile,
        system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        let permit = self.semaphore.clone().acquire_owned().await?;
        let stream = self.inner.chat_stream(profile, system, user).await?;
        Ok(Box::new(PermitStream {
            stream,
            _permit: permit,
        }))
    }
}
