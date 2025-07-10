use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};

use rand::Rng;
use segtok::segmenter::{SegmentConfig, split_single};

use crate::{Impression, PlainDescribe, Sensation, Sensor, render_template};

use crate::MemoryStore;
use crate::llm::types::Token;
use crate::llm_client::LLMClient;
use crate::neighbor::merge_neighbors;
use ollama_rs::generation::chat::ChatMessage;

/// Default prompt text for [`Wit`].
///
/// The actual narrative prompt should be provided by the caller and kept
/// outside of this crate.
const DEFAULT_PROMPT: &str = "";

/// A looping prompt witness that summarizes sensed experiences using an LLM.
#[derive(Clone)]
pub struct Wit<T = serde_json::Value> {
    llm: Arc<dyn LLMClient>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    name: String,
    prompt: String,
    delay_ms: u64,
    window_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    last_frame: Arc<Mutex<String>>,
    start_jitter_ms: u64,
}

impl<T> Wit<T> {
    /// Creates a new [`Wit`] with default configuration.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            store: None,
            name: "Wit".into(),
            prompt: DEFAULT_PROMPT.to_string(),
            delay_ms: 1000,
            window_ms: 60_000,
            window: Arc::new(Mutex::new(Vec::new())),
            last_frame: Arc::new(Mutex::new(String::new())),
            start_jitter_ms: 0,
        }
    }

    /// Sets the agent name used for logging.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Overrides the prompt template.
    pub fn prompt(mut self, template: impl Into<String>) -> Self {
        self.prompt = template.into();
        self
    }

    /// Replace the LLM client.
    pub fn llm(mut self, llm: Arc<dyn LLMClient>) -> Self {
        self.llm = llm;
        self
    }

    /// Attach a memory store used by this wit.
    pub fn memory_store(mut self, store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        self.store = Some(store);
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

    /// Sets the jitter range for the initial tick in milliseconds.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use psyche_rs::{Wit, LLMClient};
    /// use std::sync::Arc;
    ///
    /// #[derive(Clone)]
    /// struct DummyLLM;
    ///
    /// #[async_trait::async_trait]
    /// impl LLMClient for DummyLLM {
    ///     async fn chat_stream(
    ///         &self,
    ///         _msgs: &[ollama_rs::generation::chat::ChatMessage],
    ///     ) -> Result<psyche_rs::TokenStream, Box<dyn std::error::Error + Send + Sync>> {
    ///         Ok(Box::pin(futures::stream::empty()))
    ///     }
    ///     async fn embed(
    ///         &self,
    ///         _text: &str,
    ///     ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    ///         Ok(vec![0.0])
    ///     }
    /// }
    ///
    /// let llm = Arc::new(DummyLLM);
    /// let _wit = Wit::<serde_json::Value>::new(llm).start_jitter_ms(500);
    /// ```
    pub fn start_jitter_ms(mut self, ms: u64) -> Self {
        self.start_jitter_ms = ms;
        self
    }

    /// Returns a textual timeline of sensations in the current window.
    pub fn timeline(&self) -> String
    where
        T: serde::Serialize + Clone,
    {
        crate::build_timeline(&self.window)
    }
}

/// Configuration for spawning a [`Wit`] runtime loop.
struct WitRuntimeConfig<S, T> {
    llm: Arc<dyn LLMClient>,
    name: String,
    template: String,
    delay: u64,
    window_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    last_frame: Arc<Mutex<String>>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    sensors: Vec<S>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Impression<T>>>,
    abort: Option<tokio::sync::oneshot::Receiver<()>>,
    jitter: u64,
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

    fn spawn_runtime<S>(config: WitRuntimeConfig<S, T>) -> tokio::task::JoinHandle<()>
    where
        S: Sensor<T> + Send + 'static,
    {
        let WitRuntimeConfig {
            llm,
            name,
            template,
            delay,
            window_ms,
            window,
            last_frame,
            store,
            sensors,
            tx,
            mut abort,
            jitter,
        } = config;

        tokio::spawn(async move {
            debug!(agent = %name, "starting Wit thread");
            if jitter > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(jitter)).await;
            }
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
                        if pending.is_empty() {
                            continue;
                        }
                        trace!("wit loop tick");
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
                        if snapshot.is_empty() {
                            trace!("Wit skipping LLM call due to empty snapshot");
                            continue;
                        }
                        let timeline = crate::build_timeline(&window);
                        let lf = { last_frame.lock().unwrap().clone() };

                        let mut last_instant = String::new();
                        let mut last_moment = String::new();
                        for s in &snapshot {
                            let val = s.to_plain();
                            match s.kind.as_str() {
                                "instant" => last_instant = val,
                                "moment" => last_moment = val,
                                _ => {}
                            }
                        }

                        let mut neighbor_text = String::new();
                        if let Some(store) = &store {
                            let mut all = Vec::new();
                            if !last_instant.is_empty() {
                                match store.retrieve_related_impressions(&last_instant, 3).await {
                                    Ok(res) => all.extend(res),
                                    Err(e) => tracing::warn!(?e, "instant neighbor query failed"),
                                }
                            }
                            if !last_moment.is_empty() {
                                match store.retrieve_related_impressions(&last_moment, 3).await {
                                    Ok(res) => all.extend(res),
                                    Err(e) => tracing::warn!(?e, "moment neighbor query failed"),
                                }
                            }
                            let all = merge_neighbors(all, Vec::new());
                            if !all.is_empty() {
                                neighbor_text = all
                                    .iter()
                                    .map(|n| format!("- {}", n.how))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                            }
                        }

                        trace!(?timeline, "preparing prompt");
                        #[derive(serde::Serialize)]
                        struct Ctx<'a> {
                            last_frame: &'a str,
                            template: &'a str,
                            memories: &'a str,
                        }
                        let memory_section = if neighbor_text.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "Relevant memories:\n{}\nWhat's relevant among these memories?",
                                neighbor_text
                            )
                        };
                        let ctx = Ctx { last_frame: &lf, template: &timeline, memories: &memory_section };
                        let prompt = render_template(&template, &ctx).unwrap_or_else(|e| {
                            trace!(error=?e, "template render failed");
                            template.clone()
                        });
                        debug!(?prompt, "sending LLM prompt");
                        trace!("wit invoking llm");
                        let msgs = vec![ChatMessage::user(prompt)];
                        let llm_clone = llm.clone();
                        let tx_clone = tx.clone();
                        let last_frame_clone = last_frame.clone();
                        let name_clone = name.clone();
                        tokio::spawn(async move {
                            debug!(agent = %name_clone, "LLM request START {:?}", std::time::Instant::now());
                            match llm_clone.chat_stream(&msgs).await {
                                Ok(mut stream) => {
                                    let mut text = String::new();
                                    while let Some(Token { text: tok }) = stream.next().await {
                                        trace!(token = %tok, "llm token");
                                        text.push_str(&tok);
                                    }
                                    if text.trim().is_empty() {
                                        text = "No meaningful observation was made.".to_string();
                                    }
                                    debug!(agent = %name_clone, %text, "llm full response");
                                    *last_frame_clone.lock().unwrap() = text.clone();
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
                                        let _ = tx_clone.send(impressions);
                                    }
                                    debug!(agent = %name_clone, "LLM call ended");
                                    trace!("wit llm stream finished");
                                }
                                Err(err) => {
                                    trace!(?err, "llm streaming failed");
                                }
                            }
                        });
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
        })
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
        let jitter = if self.start_jitter_ms > 0 {
            let mut rng = rand::thread_rng();
            rng.gen_range(0..self.start_jitter_ms)
        } else {
            0
        };

        let config = WitRuntimeConfig {
            llm,
            name: self.name.clone(),
            template,
            delay,
            window_ms,
            window,
            last_frame,
            store: self.store.clone(),
            sensors,
            tx: tx.clone(),
            abort,
            jitter,
        };
        Self::spawn_runtime(config);

        UnboundedReceiverStream::new(rx).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{Token, TokenStream};
    use crate::llm_client::LLMClient;
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
            let words: Vec<Token> = self
                .reply
                .split_whitespace()
                .map(|w| Token {
                    text: format!("{} ", w),
                })
                .collect();
            let stream = stream::iter(words);
            Ok(Box::pin(stream))
        }

        async fn embed(
            &self,
            _text: &str,
        ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![0.0])
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
        let mut wit = Wit::new(llm).prompt("{template}").delay_ms(10);
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
                use crate::llm::types::Token;
                Ok(Box::pin(stream::once(async {
                    Token {
                        text: "done".into(),
                    }
                })))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(CountLLM(calls.clone()));
        let mut wit = Wit::new(llm).prompt("{template}").delay_ms(10);
        let sensor = TestSensor;
        let mut stream = wit.observe(vec![sensor]).await;
        let _ = stream.next().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn window_discards_old_sensations() {
        let llm = Arc::new(StaticLLM { reply: "ok".into() });
        let mut wit = Wit::new(llm)
            .prompt("{template}")
            .delay_ms(10)
            .window_ms(30);

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
        assert!(lines[0].contains("\"b\""));
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
                use crate::llm::types::Token;
                Ok(Box::pin(stream::once(async {
                    Token {
                        text: "frame".into(),
                    }
                })))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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

    #[tokio::test]
    async fn timeline_uses_local_time_and_has_fallback() {
        let llm = Arc::new(StaticLLM { reply: "".into() });
        let mut wit = Wit::new(llm).prompt("{template}").delay_ms(10);
        let sensor = TestSensor;
        let mut stream = wit.observe(vec![sensor]).await;
        let impressions = stream.next().await.unwrap();
        assert_eq!(impressions[0].how, "No meaningful observation was made.");

        let tl = wit.timeline();
        let line = tl.lines().next().unwrap();
        let ts = &line[..19];
        chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S").unwrap();
    }

    #[test]
    fn builder_sets_store() {
        use crate::InMemoryStore;
        let llm = Arc::new(StaticLLM { reply: "".into() });
        let store = Arc::new(InMemoryStore::new());
        let wit: Wit<String> = Wit::new(llm).memory_store(store.clone());
        assert!(wit.store.is_some());
    }
}
