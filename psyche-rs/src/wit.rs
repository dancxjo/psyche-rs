use std::sync::Arc;
use std::thread;

use async_trait::async_trait;
use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};

use segtok::segmenter::{SegmentConfig, split_single};

use crate::{Impression, Sensation, Sensor, Witness};

use crate::llm_client::LLMClient;
use ollama_rs::generation::chat::ChatMessage;

/// Default prompt text for [`Wit`].
const DEFAULT_PROMPT: &str = include_str!("wit_prompt.txt");

/// A looping prompt witness that summarizes sensed experiences using an LLM.
pub struct Wit<T = serde_json::Value> {
    llm: Arc<dyn LLMClient>,
    prompt: String,
    delay_ms: u64,
    min_queue: usize,
    max_queue: usize,
    window: Vec<Sensation<T>>,
}

impl<T> Wit<T> {
    /// Creates a new [`Wit`] with default configuration.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            prompt: DEFAULT_PROMPT.to_string(),
            delay_ms: 1000,
            min_queue: 1,
            max_queue: 50,
            window: Vec::new(),
        }
    }

    /// Overrides the prompt template.
    pub fn prompt(mut self, template: impl Into<String>) -> Self {
        self.prompt = template.into();
        self
    }

    /// Sets the sleep delay between ticks.
    pub fn delay_ms(mut self, delay: u64) -> Self {
        self.delay_ms = delay;
        self
    }

    /// Sets queue thresholds before invoking the LLM.
    pub fn queue(mut self, min: usize, max: usize) -> Self {
        self.min_queue = min;
        self.max_queue = max;
        self
    }

    /// Returns a textual timeline of sensations in the current window.
    pub fn timeline(&self) -> String
    where
        T: serde::Serialize,
    {
        self.window
            .iter()
            .map(|s| {
                let what = serde_json::to_string(&s.what).unwrap_or_default();
                format!("{} {} {}", s.when.to_rfc3339(), s.kind, what)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait(?Send)]
impl<T> Witness<T> for Wit<T>
where
    T: Clone + Send + 'static + serde::Serialize,
{
    async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + Send + 'static,
    {
        let (tx, rx) = unbounded_channel();
        let llm = self.llm.clone();
        let template = self.prompt.clone();
        let delay = self.delay_ms;
        let min_queue = self.min_queue;
        let max_queue = self.max_queue;
        let mut window = Vec::new();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            debug!("wit runtime started");
            rt.block_on(async move {
                let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
                let mut sensor_stream = stream::select_all(streams);
                let mut pending: Vec<Sensation<T>> = Vec::new();
                loop {
                    tokio::select! {
                        Some(batch) = sensor_stream.next() => {
                            trace!(count = batch.len(), "sensations received");
                            pending.extend(batch);
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                            if pending.len() < min_queue {
                                continue;
                            }
                            window.extend(pending.drain(..));
                            if window.len() > max_queue {
                                let excess = window.len() - max_queue;
                                window.drain(0..excess);
                            }
                            let timeline = window.iter().map(|s| {
                                let what = serde_json::to_string(&s.what).unwrap_or_default();
                                format!("{} {} {}", s.when.to_rfc3339(), s.kind, what)
                            }).collect::<Vec<_>>().join("\n");
                            debug!(?timeline, "preparing prompt");
                            let prompt = template.replace("{template}", &timeline);
                            trace!(?prompt, "sending LLM prompt");
                            let msgs = vec![ChatMessage::user(prompt)];
                            match llm.chat_stream(&msgs).await {
                                Ok(mut stream) => {
                                    let mut text = String::new();
                                    while let Some(Ok(tok)) = stream.next().await {
                                        trace!(%tok, "llm token");
                                        text.push_str(&tok);
                                    }
                                    let impressions: Vec<Impression<T>> = split_single(&text, SegmentConfig::default())
                                        .into_iter()
                                        .filter_map(|s| {
                                            let t = s.trim();
                                            if t.is_empty() {
                                                None
                                            } else {
                                                Some(Impression { how: t.to_string(), what: window.clone() })
                                            }
                                        })
                                        .collect();
                                    if !impressions.is_empty() {
                                        debug!(count = impressions.len(), "impressions generated");
                                        let _ = tx.send(impressions);
                                    }
                                }
                                Err(err) => {
                                    trace!(?err, "llm streaming failed");
                                }
                            }
                        }
                    }
                }
            });
        });

        UnboundedReceiverStream::new(rx).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_client::{LLMClient, TokenStream};
    use async_trait::async_trait;
    use futures::{StreamExt, stream};

    #[derive(Clone)]
    struct StaticLLM {
        reply: String,
    }

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            let words: Vec<String> = self
                .reply
                .split_whitespace()
                .map(|w| format!("{} ", w))
                .collect();
            let stream = stream::iter(words.into_iter().map(Result::Ok));
            Ok(Box::pin(stream))
        }
    }

    struct TestSensor;

    impl Sensor<String> for TestSensor {
        fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
            let s = Sensation {
                kind: "test".into(),
                when: chrono::Utc::now(),
                what: "ping".into(),
                source: None,
            };
            stream::once(async move { vec![s] }).boxed()
        }
    }

    #[tokio::test]
    async fn emits_impressions() {
        let llm = Arc::new(StaticLLM {
            reply: "It was fun! I loved it. Let's do it again?".into(),
        });
        let mut wit = Wit::new(llm).delay_ms(10);
        let sensor = TestSensor;
        let mut stream = wit.observe(vec![sensor]).await;
        let impressions = stream.next().await.unwrap();
        assert_eq!(impressions.len(), 3);
        assert_eq!(impressions[0].how, "It was fun!");
        assert_eq!(impressions[1].how, "I loved it.");
        assert_eq!(impressions[2].how, "Let's do it again?");
    }
}
