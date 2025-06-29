use std::sync::{Arc, Mutex};
use std::thread;

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};

use segtok::segmenter::{SegmentConfig, split_single};

use crate::{Impression, Sensation, Sensor};

use crate::llm_client::LLMClient;
use ollama_rs::generation::chat::ChatMessage;

/// Default prompt text for [`Wit`].
const DEFAULT_PROMPT: &str = include_str!("wit_prompt.txt");

/// A looping prompt witness that summarizes sensed experiences using an LLM.
pub struct Wit<T = serde_json::Value> {
    llm: Arc<dyn LLMClient>,
    prompt: String,
    delay_ms: u64,
    window_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    last_frame: Arc<Mutex<String>>,
}

impl<T> Wit<T> {
    /// Creates a new [`Wit`] with default configuration.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            prompt: DEFAULT_PROMPT.to_string(),
            delay_ms: 1000,
            window_ms: 60_000,
            window: Arc::new(Mutex::new(Vec::new())),
            last_frame: Arc::new(Mutex::new(String::new())),
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

    /// Sets the duration of the sensation window in milliseconds.
    pub fn window_ms(mut self, ms: u64) -> Self {
        self.window_ms = ms;
        self
    }

    /// Returns a textual timeline of sensations in the current window.
    pub fn timeline(&self) -> String
    where
        T: serde::Serialize,
    {
        self.window
            .lock()
            .unwrap()
            .iter()
            .map(|s| {
                let what = serde_json::to_string(&s.what).unwrap_or_default();
                format!("{} {} {}", s.when.to_rfc3339(), s.kind, what)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl<T> Wit<T>
where
    T: Clone + Send + 'static + serde::Serialize,
{
    /// Observe the provided sensors and yield impression batches.
    pub async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + Send + 'static,
    {
        self.observe_inner(sensors, None).await
    }

    /// Observe sensors and allow abortion via the provided channel.
    pub async fn observe_with_abort<S>(
        &mut self,
        sensors: Vec<S>,
        abort: tokio::sync::oneshot::Receiver<()>,
    ) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + Send + 'static,
    {
        self.observe_inner(sensors, Some(abort)).await
    }

    async fn observe_inner<S>(
        &mut self,
        sensors: Vec<S>,
        abort: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + Send + 'static,
    {
        let (tx, rx) = unbounded_channel();
        let llm = self.llm.clone();
        let template = self.prompt.clone();
        let delay = self.delay_ms;
        let window_ms = self.window_ms;
        let window = self.window.clone();
        let last_frame = self.last_frame.clone();
        let mut pending: Vec<Sensation<T>> = Vec::new();
        let mut abort = abort;

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            debug!("wit runtime started");
            rt.block_on(async move {
                let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
                let mut sensor_stream = stream::select_all(streams);
                loop {
                    tokio::select! {
                        Some(batch) = sensor_stream.next() => {
                            trace!(count = batch.len(), "sensations received");
                            pending.extend(batch);
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                            if pending.is_empty() {
                                continue;
                            }
                            {
                                let mut w = window.lock().unwrap();
                                w.extend(pending.drain(..));
                                let cutoff = chrono::Local::now() - chrono::Duration::milliseconds(window_ms as i64);
                                w.retain(|s| s.when > cutoff);
                            }
                            let snapshot = {
                                let w = window.lock().unwrap();
                                w.clone()
                            };
                            let timeline = snapshot.iter().map(|s| {
                                let what = serde_json::to_string(&s.what).unwrap_or_default();
                                format!("{} {} {}", s.when.to_rfc3339(), s.kind, what)
                            }).collect::<Vec<_>>().join("\n");
                            let lf = { last_frame.lock().unwrap().clone() };
                            trace!(?timeline, "preparing prompt");
                            let prompt = template
                                .replace("{last_frame}", &lf)
                                .replace("{template}", &timeline);
                            trace!(?prompt, "sending LLM prompt");
                            let msgs = vec![ChatMessage::user(prompt)];
                            match llm.chat_stream(&msgs).await {
                                Ok(mut stream) => {
                                    let mut text = String::new();
                                    while let Some(Ok(tok)) = stream.next().await {
                                        trace!(%tok, "llm token");
                                        text.push_str(&tok);
                                    }
                                    *last_frame.lock().unwrap() = text.clone();
                                    let impressions: Vec<Impression<T>> = split_single(&text, SegmentConfig::default())
                                        .into_iter()
                                        .filter_map(|s| {
                                            let t = s.trim();
                                            if t.is_empty() {
                                                None
                                            } else {
                                                Some(Impression { how: t.to_string(), what: snapshot.clone() })
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
                        _ = async {
                            if let Some(rx) = &mut abort {
                                let _ = rx.await;
                            } else {
                                futures::future::pending::<()>().await;
                            }
                        } => {
                            break;
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
                when: chrono::Local::now(),
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

    #[tokio::test]
    async fn avoids_duplicates_without_input() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountLLM(Arc<AtomicUsize>);

        #[async_trait]
        impl LLMClient for CountLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(stream::once(async { Ok("done".to_string()) })))
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(CountLLM(calls.clone()));
        let mut wit = Wit::new(llm).delay_ms(10);
        let sensor = TestSensor;
        let mut stream = wit.observe(vec![sensor]).await;
        let _ = stream.next().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn window_discards_old_sensations() {
        let llm = Arc::new(StaticLLM { reply: "ok".into() });
        let mut wit = Wit::new(llm).delay_ms(10).window_ms(30);

        struct TwoEventSensor;

        impl Sensor<String> for TwoEventSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                use async_stream::stream;
                let s = stream! {
                    yield vec![Sensation {
                        kind: "test".into(),
                        when: chrono::Local::now(),
                        what: "a".into(),
                        source: None,
                    }];
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    yield vec![Sensation {
                        kind: "test".into(),
                        when: chrono::Local::now(),
                        what: "b".into(),
                        source: None,
                    }];
                };
                Box::pin(s)
            }
        }

        let sensor = TwoEventSensor;
        let mut stream = wit.observe(vec![sensor]).await;
        let _ = stream.next().await;
        let _ = stream.next().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let tl = wit.timeline();
        let lines: Vec<_> = tl.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("b"));
    }

    #[tokio::test]
    async fn includes_last_frame_in_prompt() {
        #[derive(Clone)]
        struct RecLLM {
            prompts: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl LLMClient for RecLLM {
            async fn chat_stream(
                &self,
                msgs: &[ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.prompts.lock().unwrap().push(msgs[0].content.clone());
                Ok(Box::pin(stream::once(async { Ok("frame".to_string()) })))
            }
        }

        struct TwoBatch;

        impl Sensor<String> for TwoBatch {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                use async_stream::stream;
                let s = stream! {
                    yield vec![Sensation {
                        kind: "test".into(),
                        when: chrono::Local::now(),
                        what: "a".into(),
                        source: None,
                    }];
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    yield vec![Sensation {
                        kind: "test".into(),
                        when: chrono::Local::now(),
                        what: "b".into(),
                        source: None,
                    }];
                };
                Box::pin(s)
            }
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let llm = Arc::new(RecLLM {
            prompts: prompts.clone(),
        });
        let mut wit = Wit::new(llm).delay_ms(10).prompt("{last_frame}:{template}");
        let sensor = TwoBatch;
        let mut stream = wit.observe(vec![sensor]).await;
        let _ = stream.next().await;
        let _ = stream.next().await;
        let data = prompts.lock().unwrap();
        assert!(data.len() >= 2);
        assert!(data[1].contains("frame"));
    }
}
