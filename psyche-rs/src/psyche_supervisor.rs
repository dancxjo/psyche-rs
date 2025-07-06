use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tokio::sync::broadcast;
use tracing::{debug, error};

use crate::{Genius, ThreadLocalContext, launch_genius};

/// Key wrapper allowing [`Arc`] pointers to be used in a [`HashMap`]
/// by comparing the underlying pointer value.
#[derive(Clone)]
struct ArcKey<G: Genius>(Arc<G>);

impl<G: Genius> ArcKey<G> {
    fn new(inner: Arc<G>) -> Self {
        Self(inner)
    }
}

impl<G: Genius> PartialEq for ArcKey<G> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<G: Genius> Eq for ArcKey<G> {}

impl<G: Genius> Hash for ArcKey<G> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

struct Entry<G: Genius> {
    genius: Arc<G>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    core: Option<usize>,
}

/// Supervises a set of [`Genius`] threads.
///
/// Each genius is spawned using [`launch_genius`] and restarted if the thread
/// stops unexpectedly. Threads are expected to create a [`ThreadLocalContext`]
/// with dedicated LLM and memory clients.
///
/// ```no_run
/// use std::sync::Arc;
/// use tokio::sync::mpsc::unbounded_channel;
/// use psyche_rs::{PsycheSupervisor, QuickGenius};
///
/// #[tokio::main]
/// async fn main() {
///     let (out_tx, _out_rx) = unbounded_channel();
///     let (quick, _tx) = QuickGenius::with_capacity(1, out_tx);
///     let quick = Arc::new(quick);
///     let mut sup = PsycheSupervisor::new();
///     sup.add_genius(quick);
///     sup.start(None);
///     sup.shutdown().await;
/// }
/// ```
pub struct PsycheSupervisor<G: Genius> {
    entries: HashMap<ArcKey<G>, Entry<G>>,
    stop_tx: broadcast::Sender<()>,
}

impl<G> PsycheSupervisor<G>
where
    G: Genius,
{
    /// Create an empty supervisor.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self {
            entries: HashMap::new(),
            stop_tx: tx,
        }
    }

    /// Register a genius to be supervised.
    pub fn add_genius(&mut self, genius: Arc<G>) {
        self.add_genius_on_core(genius, None);
    }

    /// Register a genius pinned to a specific CPU core.
    pub fn add_genius_on_core(&mut self, genius: Arc<G>, core: Option<usize>) {
        let key = ArcKey::new(genius.clone());
        self.entries.insert(
            key,
            Entry {
                genius,
                handle: Arc::new(Mutex::new(None)),
                core,
            },
        );
    }

    /// Spawn all registered genii and start monitoring their threads.
    pub fn start(&mut self, delay_ms: Option<u64>) {
        for entry in self.entries.values_mut() {
            let handle = launch_genius(entry.genius.clone(), delay_ms, entry.core);
            *entry.handle.lock().unwrap() = Some(handle);
            Self::monitor(
                entry.genius.clone(),
                Arc::clone(&entry.handle),
                self.stop_tx.subscribe(),
                delay_ms,
                entry.core,
            );
        }
    }

    fn monitor(
        genius: Arc<G>,
        handle: Arc<Mutex<Option<JoinHandle<()>>>>,
        mut stop_rx: broadcast::Receiver<()>,
        delay_ms: Option<u64>,
        core: Option<usize>,
    ) {
        tokio::spawn(async move {
            loop {
                let join_handle = handle.lock().unwrap().take().expect("missing handle");
                let mut join_future = tokio::task::spawn_blocking(move || join_handle.join());
                tokio::select! {
                    _ = stop_rx.recv() => {
                        let _ = join_future.await;
                        break;
                    }
                    res = &mut join_future => {
                        match res {
                            Ok(Ok(())) => debug!(name=%genius.name(), "genius thread exited"),
                            Ok(Err(_)) => error!(name=%genius.name(), "genius thread panicked"),
                            Err(e) => error!(?e, "join task failed"),
                        }
                    }
                }

                if stop_rx.try_recv().is_ok() {
                    break;
                }

                let new_handle = launch_genius(genius.clone(), delay_ms, core);
                *handle.lock().unwrap() = Some(new_handle);
            }
        });
    }

    /// Broadcast a stop signal and wait for all threads to finish.
    pub async fn shutdown(&mut self) {
        let _ = self.stop_tx.send(());
        for entry in self.entries.values() {
            if let Some(handle) = entry.handle.lock().unwrap().take() {
                let _ = tokio::task::spawn_blocking(move || handle.join()).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::TokenStream;
    use async_trait::async_trait;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::Duration;

    struct CountGenius {
        count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Genius for CountGenius {
        type Input = ();
        type Output = ();

        fn name(&self) -> &'static str {
            "count"
        }

        async fn call(&self, _input: Self::Input) -> TokenStream {
            use crate::llm::types::Token;
            use futures::stream;
            stream::once(async {
                Token {
                    text: String::new(),
                }
            })
            .boxed()
        }

        async fn run(&self) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn restarts_exited_genius() {
        let count = Arc::new(AtomicUsize::new(0));
        let genius = Arc::new(CountGenius {
            count: count.clone(),
        });
        let mut sup = PsycheSupervisor::new();
        sup.add_genius(genius);
        sup.start(None); // thread exits immediately
        tokio::time::sleep(Duration::from_millis(30)).await;
        sup.shutdown().await;
        assert!(count.load(Ordering::SeqCst) > 1);
    }

    struct PanicGenius {
        count: Arc<AtomicUsize>,
        first: AtomicUsize,
    }

    #[async_trait]
    impl Genius for PanicGenius {
        type Input = ();
        type Output = ();

        fn name(&self) -> &'static str {
            "panic"
        }

        async fn call(&self, _input: Self::Input) -> TokenStream {
            use crate::llm::types::Token;
            use futures::stream;
            stream::once(async {
                Token {
                    text: String::new(),
                }
            })
            .boxed()
        }

        async fn run(&self) {
            if self.first.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("boom");
            }
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn restarts_panicking_genius() {
        let count = Arc::new(AtomicUsize::new(0));
        let genius = Arc::new(PanicGenius {
            count: count.clone(),
            first: AtomicUsize::new(0),
        });
        let mut sup = PsycheSupervisor::new();
        sup.add_genius(genius);
        sup.start(None);
        tokio::time::sleep(Duration::from_millis(30)).await;
        sup.shutdown().await;
        assert!(count.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn adds_genius_with_core() {
        let count = Arc::new(AtomicUsize::new(0));
        let genius = Arc::new(CountGenius {
            count: count.clone(),
        });
        let mut sup = PsycheSupervisor::new();
        sup.add_genius_on_core(genius, Some(0));
        sup.start(None);
        tokio::time::sleep(Duration::from_millis(10)).await;
        sup.shutdown().await;
        assert!(count.load(Ordering::SeqCst) >= 1);
    }
}
