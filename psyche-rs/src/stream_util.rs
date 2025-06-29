use futures::Stream;
use tokio::sync::OwnedSemaphorePermit;

/// Stream wrapper that releases a [`Semaphore`] permit when the inner stream
/// ends or the wrapper is dropped.
///
/// This is used by [`FairLLM`](crate::FairLLM) to ensure concurrency permits are
/// returned even if a client drops the token stream early.
pub(crate) struct ReleasingStream<S> {
    inner: S,
    permit: Option<OwnedSemaphorePermit>,
}

impl<S> ReleasingStream<S> {
    pub fn new(inner: S, permit: OwnedSemaphorePermit) -> Self {
        Self {
            inner,
            permit: Some(permit),
        }
    }
}

impl<S> Drop for ReleasingStream<S> {
    fn drop(&mut self) {
        let _ = self.permit.take();
    }
}

impl<S> Stream for ReleasingStream<S>
where
    S: Stream + Unpin,
{
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let poll = std::pin::Pin::new(&mut self.inner).poll_next(cx);
        if let std::task::Poll::Ready(None) = poll {
            let _ = self.permit.take();
        }
        poll
    }
}
