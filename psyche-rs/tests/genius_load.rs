use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::StreamExt;
use psyche_rs::{Genius, GeniusSender, TokenStream, bounded_channel, launch_genius};

struct DummyGenius {
    rx: Mutex<Option<tokio::sync::mpsc::Receiver<Instant>>>,
    count: Arc<AtomicUsize>,
    latencies: Arc<Mutex<Vec<u128>>>,
}

impl DummyGenius {
    fn new(
        rx: tokio::sync::mpsc::Receiver<Instant>,
        latencies: Arc<Mutex<Vec<u128>>>,
        count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            rx: Mutex::new(Some(rx)),
            latencies,
            count,
        }
    }
}

#[async_trait]
impl Genius for DummyGenius {
    type Input = Instant;
    type Output = ();

    fn name(&self) -> &'static str {
        "dummy"
    }

    async fn call(&self, _input: Self::Input) -> TokenStream {
        use psyche_rs::Token;
        futures::stream::empty::<Token>().boxed()
    }

    async fn run(&self) {
        let mut rx = self.rx.lock().unwrap().take().unwrap();
        while let Some(start) = rx.recv().await {
            let lat = start.elapsed().as_millis();
            self.count.fetch_add(1, Ordering::SeqCst);
            self.latencies.lock().unwrap().push(lat);
            tracing::info!(latency_ms = lat, thread = ?std::thread::current().id(), "dummy_latency");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn load_test_throughput() {
    let latencies = Arc::new(Mutex::new(Vec::new()));
    let processed = Arc::new(AtomicUsize::new(0));

    let mut senders: Vec<GeniusSender<Instant>> = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (tx, rx) = bounded_channel(8, "dummy");
        let genius = Arc::new(DummyGenius::new(rx, latencies.clone(), processed.clone()));
        senders.push(tx);
        handles.push(launch_genius(genius, Some(0)));
    }

    let start = Instant::now();
    for _ in 0..100 {
        for tx in &senders {
            tx.send(Instant::now());
        }
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    for handle in handles {
        handle.thread().unpark();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let count = processed.load(Ordering::SeqCst);
    let throughput = count as f64 / elapsed;

    let lats = latencies.lock().unwrap();
    let avg = if count > 0 {
        lats.iter().sum::<u128>() as f64 / count as f64
    } else {
        0.0
    };

    tracing::info!(throughput, avg_latency_ms = avg);
    assert!(throughput > 0.0);
}
