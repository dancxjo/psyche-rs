use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tracing::{debug, error, info};

use crate::genius::Genius;

/// Launches a [`Genius`] on its own OS thread.
///
/// The genius runs inside a single-threaded Tokio runtime. If `delay_ms` is
/// provided, the runtime sleeps for that duration after each iteration of
/// [`Genius::run`]. The returned [`JoinHandle`] can be detached or joined
/// by the caller.
pub fn launch_genius<G>(genius: Arc<G>, delay_ms: Option<u64>) -> JoinHandle<()>
where
    G: Genius,
{
    thread::spawn(move || {
        info!(name = %genius.name(), "starting genius thread");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build thread-local runtime");

        rt.block_on(async {
            loop {
                genius.run().await;
                if let Some(ms) = delay_ms {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                } else {
                    break;
                }
            }
        });

        debug!(name = %genius.name(), "stopping genius thread");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestGenius {
        count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Genius for TestGenius {
        type Input = ();
        type Output = ();

        fn name(&self) -> &'static str {
            "Test"
        }

        async fn call(&self, _input: Self::Input) -> crate::llm::types::TokenStream {
            use crate::llm::types::Token;
            stream::once(async { Token { text: "".into() } }).boxed()
        }

        async fn run(&self) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn launches_thread_and_runs() {
        let count = Arc::new(AtomicUsize::new(0));
        let genius = Arc::new(TestGenius {
            count: count.clone(),
        });
        let handle = launch_genius(genius, Some(1));
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(count.load(Ordering::SeqCst) >= 1);
        drop(handle);
    }
}
