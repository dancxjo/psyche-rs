/// Guard that aborts a task when dropped.
///
/// Wraps a `JoinHandle` and aborts the task if the guard is dropped before
/// the handle is taken.
///
/// # Example
/// ```ignore
/// use psyche_rs::AbortGuard;
/// let guard = AbortGuard::new(tokio::spawn(async { /* work */ }));
/// drop(guard); // task is aborted here
/// ```
pub struct AbortGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl AbortGuard {
    /// Create a new guard from a [`JoinHandle`].
    pub fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    /// Remove and return the inner handle without aborting.
    #[allow(dead_code)]
    pub fn into_inner(mut self) -> Option<tokio::task::JoinHandle<()>> {
        self.handle.take()
    }
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn aborts_task_on_drop() {
        // Given a task waiting on a channel
        let (tx, rx) = oneshot::channel::<()>();
        {
            // When the guard is dropped the task should be aborted
            let _guard = AbortGuard::new(tokio::spawn(async move {
                let _ = rx.await;
            }));
            // dropping here aborts the task
        }
        // give the runtime a moment to process the abort
        tokio::task::yield_now().await;
        // Then sending fails because the receiver was dropped
        assert!(tx.send(()).is_err());
    }
}
