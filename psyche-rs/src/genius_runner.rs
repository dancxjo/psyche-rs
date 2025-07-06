use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tracing::{debug, error, info};

use core_affinity::CoreId;

use crate::ThreadLocalContext;
use crate::genius::Genius;

/// Launches a [`Genius`] on its own OS thread.
///
/// Each thread should create its own [`ThreadLocalContext`] so that LLM clients
/// and memory stores are scoped per-thread. The genius runs inside a
/// single-threaded Tokio runtime. If `delay_ms` is provided, the runtime
/// sleeps for that duration after each iteration of [`Genius::run`]. The
/// returned [`JoinHandle`] can be detached or joined by the caller.
pub fn launch_genius<G>(
    genius: Arc<G>,
    delay_ms: Option<u64>,
    core: Option<usize>,
) -> JoinHandle<()>
where
    G: Genius,
{
    thread::spawn(move || {
        if let Some(core_index) = core {
            if let Some(ids) = core_affinity::get_core_ids() {
                if let Some(id) = ids.into_iter().find(|c| c.id == core_index) {
                    let _ = core_affinity::set_for_current(id);
                }
            }
        }
        info!(name = %genius.name(), "starting genius thread");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
        }));

        match result {
            Ok(()) => debug!(name = %genius.name(), "stopping genius thread"),
            Err(e) => {
                if let Some(s) = e.downcast_ref::<&str>() {
                    error!(name = %genius.name(), panic = %s, "genius panicked");
                } else if let Some(s) = e.downcast_ref::<String>() {
                    error!(name = %genius.name(), panic = %s, "genius panicked");
                } else {
                    error!(name = %genius.name(), "genius panicked");
                }
            }
        }
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
        let handle = launch_genius(genius, Some(1), None);
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(count.load(Ordering::SeqCst) >= 1);
        drop(handle);
    }
}
