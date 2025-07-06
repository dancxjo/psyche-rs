use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use psyche_rs::{
    ActionResult, InMemoryStore, Intention, LLMClient, MemoryStore, Motor, MotorError, Psyche,
    Sensation, Sensor, StoredImpression, StoredSensation, Will,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::time::Duration;

#[derive(Default)]
struct SlowStore {
    inner: InMemoryStore,
    delay: u64,
}

impl SlowStore {
    fn new(delay: u64) -> Self {
        Self {
            inner: InMemoryStore::new(),
            delay,
        }
    }
}

#[async_trait]
impl MemoryStore for SlowStore {
    async fn store_sensation(&self, s: &StoredSensation) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.delay)).await;
        self.inner.store_sensation(s).await
    }

    async fn store_impression(&self, i: &StoredImpression) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.delay)).await;
        self.inner.store_impression(i).await
    }

    async fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        self.inner
            .add_lifecycle_stage(impression_id, stage, detail)
            .await
    }

    async fn retrieve_related_impressions(
        &self,
        query: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        self.inner.retrieve_related_impressions(query, top_k).await
    }

    async fn fetch_recent_impressions(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        self.inner.fetch_recent_impressions(limit).await
    }

    async fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        std::collections::HashMap<String, String>,
    )> {
        self.inner.load_full_impression(impression_id).await
    }
}

#[derive(Clone)]
struct MultiActionLLM;

#[async_trait]
impl LLMClient for MultiActionLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        use psyche_rs::Token;
        let toks = vec![
            Token {
                text: "<log>".into(),
            },
            Token { text: "1".into() },
            Token {
                text: "</log>".into(),
            },
            Token {
                text: "<log>".into(),
            },
            Token { text: "2".into() },
            Token {
                text: "</log>".into(),
            },
        ];
        Ok(Box::pin(stream::iter(toks)))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![0.0])
    }
}

struct DoubleSensor;

impl Sensor<String> for DoubleSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        use async_stream::stream;
        let s = stream! {
            yield vec![Sensation {
                kind: "t".into(),
                when: chrono::Local::now(),
                what: "foo".into(),
                source: None,
            }];
            tokio::time::sleep(Duration::from_millis(20)).await;
            yield vec![Sensation {
                kind: "t".into(),
                when: chrono::Local::now(),
                what: "bar".into(),
                source: None,
            }];
        };
        Box::pin(s)
    }
}

struct CountMotor(Arc<AtomicUsize>);

#[async_trait]
impl Motor for CountMotor {
    fn description(&self) -> &'static str {
        "count"
    }
    fn name(&self) -> &'static str {
        "log"
    }
    async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ActionResult::default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistence_does_not_block_main_loop() {
    let llm = Arc::new(MultiActionLLM);
    let will = Will::new(llm).delay_ms(10).motor("log", "count");
    let store = Arc::new(SlowStore::new(50));
    let count = Arc::new(AtomicUsize::new(0));
    let psyche = Psyche::new()
        .sensor(DoubleSensor)
        .will(will)
        .motor(CountMotor(count.clone()))
        .memory(store);

    let _ = tokio::time::timeout(Duration::from_millis(200), psyche.run()).await;
    assert!(count.load(Ordering::SeqCst) >= 2);
}
