use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, error, trace, warn};

use once_cell::sync::OnceCell;
use regex::Regex;

use crate::MemoryStore;
use crate::llm_client::LLMClient;
use crate::timeline::build_timeline_from_slice;
use crate::{Intention, Motor, PlainDescribe, Sensation, Sensor, render_template};
use ollama_rs::generation::chat::ChatMessage;

/// Placeholder prompt text for [`Will`].
///
/// Applications should provide their own narrative prompt text when
/// constructing a [`Will`] instance.
const DEFAULT_PROMPT: &str = "";
/// Maximum number of sensations retained in the window.
const WINDOW_CAP: usize = 1000;

/// Returns a prefix of `s` that fits within `max_bytes` without splitting UTF-8
/// characters.
///
/// If `max_bytes` does not land on a char boundary, the prefix is truncated to
/// the previous valid boundary and a warning is emitted.
pub fn safe_prefix(s: &str, max_bytes: usize) -> &str {
    if let Some(slice) = s.get(..max_bytes) {
        return slice;
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    if end != max_bytes {
        warn!(index = max_bytes, "Invalid char boundary, truncating slice");
    }
    &s[..end]
}

/// Build a case-insensitive [`Regex`] matching any of the provided motor names
/// as an opening XML tag.
#[allow(dead_code)]
pub fn build_motor_regex(motors: &[MotorDescription]) -> Regex {
    if motors.is_empty() {
        // Match nothing when no motors are registered.
        return Regex::new("$^").expect("valid regex");
    }
    let names = motors
        .iter()
        .map(|m| regex::escape(&m.name))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(r"(?i)<(?:{})[^>]*>", names)).expect("valid regex")
}

/// Description of an available motor.
#[derive(Clone)]
pub struct MotorDescription {
    pub name: String,
    pub description: String,
}

/// A looping controller that turns sensations into motor actions using an LLM.
pub struct Will<T = serde_json::Value> {
    llm: Arc<dyn LLMClient>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    name: String,
    prompt: String,
    /// Sleep duration between cycles in milliseconds.
    ///
    /// Defaults to `0` so the Will loops continuously.
    delay_ms: u64,
    window_ms: u64,
    /// Minimum delay between LLM calls when the snapshot is unchanged.
    min_llm_interval_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    motors: Arc<[MotorDescription]>,
    /// Precomputed list of motor descriptions formatted as lines.
    motor_text: String,
    motor_regex: OnceCell<Regex>,
    latest_instant: Arc<Mutex<String>>,
    latest_moment: Arc<Mutex<String>>,
    thoughts_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>>,
}

/// Configuration for spawning a [`Will`] runtime loop.
struct WillRuntimeConfig<S, T> {
    llm: Arc<dyn LLMClient>,
    name: String,
    template: String,
    delay: u64,
    window_ms: u64,
    min_llm_interval_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    motor_text: String,
    latest_instant_store: Arc<Mutex<String>>,
    latest_moment_store: Arc<Mutex<String>>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    thoughts_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>>,
    sensors: Vec<S>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Intention>>,
    abort: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl<T> Will<T> {
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            store: None,
            name: "Will".into(),
            prompt: DEFAULT_PROMPT.to_string(),
            // default to zero so the Will loops continuously
            delay_ms: 0,
            window_ms: 60_000,
            min_llm_interval_ms: 0,
            window: Arc::new(Mutex::new(Vec::new())),
            motors: Arc::from(Vec::<MotorDescription>::new()),
            motor_text: String::new(),
            motor_regex: OnceCell::new(),
            latest_instant: Arc::new(Mutex::new(String::new())),
            latest_moment: Arc::new(Mutex::new(String::new())),
            thoughts_tx: None,
        }
    }

    /// Sets the agent name used for logging.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn prompt(mut self, template: impl Into<String>) -> Self {
        self.prompt = template.into();
        self
    }

    /// Replace the LLM client.
    pub fn llm(mut self, llm: Arc<dyn LLMClient>) -> Self {
        self.llm = llm;
        self
    }

    /// Attach a memory store used by this will.
    pub fn memory_store(mut self, store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the sleep duration between cycles.
    ///
    /// A value of `0` makes the Will loop continuously.
    pub fn delay_ms(mut self, delay: u64) -> Self {
        self.delay_ms = delay;
        self
    }

    pub fn window_ms(mut self, ms: u64) -> Self {
        self.window_ms = ms;
        self
    }

    /// Sets the minimum interval between LLM calls when the snapshot hasn't changed.
    pub fn min_llm_interval_ms(mut self, ms: u64) -> Self {
        self.min_llm_interval_ms = ms;
        self
    }

    pub fn thoughts(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        self.thoughts_tx = Some(tx);
        self
    }

    pub fn motor(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        let mut list: Vec<MotorDescription> = self.motors.as_ref().to_vec();
        list.push(MotorDescription {
            name: name.into(),
            description: description.into(),
        });
        self.motors = Arc::from(list);
        self.rebuild_motor_text();
        let _ = self.motor_regex.take();
        debug!(motor_name = %self.motors.last().unwrap().name, "Will registered motor");
        self
    }

    pub fn register_motor(&mut self, motor: &dyn Motor) -> &mut Self {
        let mut list: Vec<MotorDescription> = self.motors.as_ref().to_vec();
        list.push(MotorDescription {
            name: motor.name().to_string(),
            description: motor.description().to_string(),
        });
        self.motors = Arc::from(list);
        self.rebuild_motor_text();
        let _ = self.motor_regex.take();
        debug!(motor_name = %motor.name(), "Will registered motor");
        self
    }

    /// Recompute the joined motor description string.
    fn rebuild_motor_text(&mut self) {
        self.motor_text = self
            .motors
            .iter()
            .map(|m| format!("{}: {}", m.name, m.description))
            .collect::<Vec<_>>()
            .join("\n");
    }

    /// Returns the cached motor description list.
    ///
    /// Each line is formatted as `"name: description"`.
    pub fn motor_text(&self) -> &str {
        &self.motor_text
    }

    /// Access the underlying sensation window.
    pub fn window_arc(&self) -> Arc<Mutex<Vec<Sensation<T>>>> {
        self.window.clone()
    }

    /// Latest instant recorded by this Will.
    pub fn latest_instant_arc(&self) -> Arc<Mutex<String>> {
        self.latest_instant.clone()
    }

    /// Latest moment recorded by this Will.
    pub fn latest_moment_arc(&self) -> Arc<Mutex<String>> {
        self.latest_moment.clone()
    }

    pub fn timeline(&self) -> String
    where
        T: serde::Serialize + Clone,
    {
        crate::build_timeline(&self.window)
    }

    /// Returns `true` if `output` contains any valid motor action tag.
    ///
    /// The check is performed by parsing `output` as XML using [`quick_xml`].
    /// Element names are compared case-insensitively against the known motor
    /// list.
    pub fn contains_motor_action(&self, output: &str) -> bool {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let names: std::collections::HashSet<String> = self
            .motors
            .iter()
            .map(|m| m.name.to_ascii_lowercase())
            .collect();

        let mut reader = Reader::from_str(output);
        reader.trim_text(true);
        let mut buf = Vec::new();

        loop {
            let before = reader.buffer_position();
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let after = reader.buffer_position();
                    if output[before..after].contains('>') {
                        if let Ok(tag) = std::str::from_utf8(e.name().as_ref()) {
                            if names.contains(&tag.to_ascii_lowercase()) {
                                return true;
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        false
    }

    pub async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Intention>>
    where
        T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
        S: Sensor<T> + Send + 'static,
    {
        self.observe_with_abort(sensors, None).await
    }

    /// Observe sensors and allow abortion via the provided channel.
    pub async fn observe_with_abort<S>(
        &mut self,
        sensors: Vec<S>,
        abort: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> BoxStream<'static, Vec<Intention>>
    where
        T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
        S: Sensor<T> + Send + 'static,
    {
        let (tx, rx) = unbounded_channel();
        let config = WillRuntimeConfig {
            llm: self.llm.clone(),
            name: self.name.clone(),
            template: self.prompt.clone(),
            delay: self.delay_ms,
            window_ms: self.window_ms,
            min_llm_interval_ms: self.min_llm_interval_ms,
            window: self.window.clone(),
            motor_text: self.motor_text.clone(),
            latest_instant_store: self.latest_instant.clone(),
            latest_moment_store: self.latest_moment.clone(),
            store: self.store.clone(),
            thoughts_tx: self.thoughts_tx.clone(),
            sensors,
            tx: tx.clone(),
            abort,
        };
        Self::spawn_runtime(config);

        UnboundedReceiverStream::new(rx).boxed()
    }

    /// Spawns the async runtime driving Will's perception and decision loop.
    ///
    /// The returned task listens for shutdown signals or an optional abort
    /// channel and ensures any in-flight LLM stream is aborted before exiting.
    fn spawn_runtime<S>(config: WillRuntimeConfig<S, T>) -> tokio::task::JoinHandle<()>
    where
        T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
        S: Sensor<T> + Send + 'static,
    {
        let WillRuntimeConfig {
            llm,
            name,
            template,
            delay,
            window_ms,
            min_llm_interval_ms,
            window,
            motor_text,
            latest_instant_store,
            latest_moment_store,
            store,
            thoughts_tx,
            sensors,
            tx,
            mut abort,
            ..
        } = config;

        tokio::spawn(async move {
            debug!(agent = %name, "starting Will thread");
            let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
            let mut sensor_stream = stream::select_all(streams);
            let mut pending: Vec<Sensation<T>> = Vec::new();
            let mut last_hash: Option<Vec<u8>> = None;
            let mut last_time: Option<std::time::Instant> = None;

            loop {
                tokio::select! {
                    _ = async {
                        if let Some(rx) = &mut abort {
                            let _ = rx.await;
                        } else {
                            futures::future::pending::<()>().await;
                        }
                    } => {
                        debug!(agent=%name, "Will runtime aborted");
                        break;
                    }
                    _ = crate::shutdown_signal() => {
                        debug!(agent=%name, "Will runtime shutting down");
                        break;
                    }
                    Some(batch) = sensor_stream.next() => {
                        trace!(count = batch.len(), "sensations received");
                        pending.extend(batch);
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                        if pending.is_empty() {
                            continue;
                        }

                        trace!("will loop tick");
                        let snapshot = {
                            let mut w = window.lock().unwrap();
                            w.extend(pending.drain(..));
                            let cutoff = chrono::Local::now() - chrono::Duration::milliseconds(window_ms as i64);
                            w.retain(|s| s.when > cutoff);
                            if w.len() > WINDOW_CAP {
                                let drop = w.len() - WINDOW_CAP;
                                w.drain(0..drop);
                                trace!(drop, "Will truncated window");
                            }
                            w.clone()
                        };

                        if snapshot.is_empty() {
                            trace!("Will skipping LLM call due to empty snapshot");
                            continue;
                        }

                        trace!(snapshot_len = snapshot.len(), "Will captured snapshot");

                        let snapshot_hash = match serde_json::to_vec(&snapshot) {
                            Ok(bytes) => {
                                use sha2::{Digest, Sha256};
                                let mut hasher = Sha256::new();
                                hasher.update(&bytes);
                                hasher.finalize().to_vec()
                            }
                            Err(e) => {
                                warn!(error=?e, "snapshot serialization failed");
                                Vec::new()
                            }
                        };
                        let now = std::time::Instant::now();
                        if let (Some(h), Some(t)) = (&last_hash, last_time) {
                            if *h == snapshot_hash && now.duration_since(t) < std::time::Duration::from_millis(min_llm_interval_ms) {
                                trace!("Will throttling duplicate snapshot");
                                continue;
                            }
                        }
                        last_hash = Some(snapshot_hash);
                        last_time = Some(now);

                        let situation = build_timeline_from_slice(&snapshot);



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

                        *latest_instant_store.lock().unwrap() = last_instant.clone();
                        *latest_moment_store.lock().unwrap() = last_moment.clone();

                        let mut neighbor_text = String::new();
                        if let Some(store) = &store {
                            let mut all = Vec::new();
                            if !last_instant.is_empty() {
                                match store.retrieve_related_impressions(&last_instant, 3).await {
                                    Ok(res) => all.extend(res),
                                    Err(e) => warn!(?e, "instant neighbor query failed"),
                                }
                            }
                            if !last_moment.is_empty() {
                                match store.retrieve_related_impressions(&last_moment, 3).await {
                                    Ok(res) => all.extend(res),
                                    Err(e) => warn!(?e, "moment neighbor query failed"),
                                }
                            }
                            let all = crate::neighbor::merge_neighbors(all, Vec::new());
                            if !all.is_empty() {
                                neighbor_text = all
                                    .iter()
                                    .map(|n| format!("- {}", n.how))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                            }
                        }

                        #[derive(serde::Serialize)]
                        struct Ctx<'a> {
                            situation: &'a str,
                            motors: &'a str,
                            latest_instant: &'a str,
                            latest_moment: &'a str,
                            memories: &'a str,
                        }
                        let memory_section = if neighbor_text.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "Relevant memories:\n{}\nWhat's relevant among this?",
                                neighbor_text
                            )
                        };
                        let ctx = Ctx {
                            situation: &situation,
                            motors: &motor_text,
                            latest_instant: &last_instant,
                            latest_moment: &last_moment,
                            memories: &memory_section,
                        };
                        let prompt = render_template(&template, &ctx).unwrap_or_else(|e| {
                            warn!(error=?e, "template render failed");
                            template.clone()
                        });

                        debug!(%prompt, "Will generated prompt");
                        trace!("will invoking llm");

                        let msgs = vec![ChatMessage::user(prompt)];
                        match llm.chat_stream(&msgs).await {
                            Ok(stream) => {
                                crate::llm_parser::drive_llm_stream(
                                    &name,
                                    stream,
                                    window.clone(),
                                    tx.clone(),
                                    thoughts_tx.clone(),
                                )
                                .await;
                            }
                            Err(err) => {
                                error!(?err, "llm streaming failed");
                            }
                        }
                    }
                }
            }
            debug!(agent=%name, "Will thread exiting");
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{Token, TokenStream};
    use crate::{
        ActionResult, Impression, Intention, MotorError, Sensor, llm_client::LLMClient,
        test_helpers::StaticLLM,
    };
    use ollama_rs::generation::chat::ChatMessage;
    use std::sync::Arc;

    #[test]
    fn detects_motor_tag() {
        // Given a Will with a "speak" motor
        let llm = Arc::new(StaticLLM::new(""));
        let mut will = Will::<serde_json::Value>::new(llm);
        will = will.motor("speak", "speak words");

        // When the output contains a <speak> tag
        let out = "Thinking <speak mood=\"happy\">hi</speak>";

        // Then the tag is detected
        assert!(will.contains_motor_action(out));
        assert!(!will.contains_motor_action("no tag here"));
    }

    #[test]
    fn tag_names_are_case_insensitive() {
        // Given a Will with a "Write" motor
        let llm = Arc::new(StaticLLM::new(""));
        let will = Will::<serde_json::Value>::new(llm).motor("Write", "");

        // Then mixed case tags are still detected
        assert!(will.contains_motor_action("<write/>"));
        assert!(will.contains_motor_action("<WRITE></WRITE>"));
    }

    #[test]
    fn ignores_unknown_tags() {
        // Given a Will with a single motor
        let llm = Arc::new(StaticLLM::new(""));
        let will = Will::<serde_json::Value>::new(llm).motor("speak", "");

        // When the output contains an unknown element
        assert!(!will.contains_motor_action("<unknown/>"));
    }

    #[test]
    fn handles_self_closing_tags() {
        // Given a Will with a "draw" motor
        let llm = Arc::new(StaticLLM::new(""));
        let will = Will::<serde_json::Value>::new(llm).motor("draw", "");

        // Then self closing tags are detected
        assert!(will.contains_motor_action("<draw/>"));
    }

    #[test]
    fn invalid_xml_is_ignored() {
        // Given a Will with a "speak" motor
        let llm = Arc::new(StaticLLM::new(""));
        let will = Will::<serde_json::Value>::new(llm).motor("speak", "");

        // When the xml is malformed no motor should be detected
        assert!(!will.contains_motor_action("<speak"));
    }

    struct DummyMotor;
    #[async_trait::async_trait]
    impl Motor for DummyMotor {
        fn description(&self) -> &'static str {
            "test"
        }
        fn name(&self) -> &'static str {
            "dum"
        }
        async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
            Ok(ActionResult::default())
        }
    }

    #[test]
    fn register_motor_updates_list() {
        let llm = Arc::new(StaticLLM::new(""));
        let mut will = Will::<serde_json::Value>::new(llm);
        let motor = DummyMotor;
        will.register_motor(&motor);
        assert!(will.contains_motor_action("<dum></dum>"));
    }

    #[test]
    fn precomputed_motor_text_updates() {
        let llm = Arc::new(StaticLLM::new(""));
        let mut will = Will::<serde_json::Value>::new(llm);
        assert_eq!(will.motor_text(), "");
        will = will.motor("speak", "speak");
        assert_eq!(will.motor_text(), "speak: speak");
        let motor = DummyMotor;
        will.register_motor(&motor);
        assert_eq!(will.motor_text(), "speak: speak\ndum: test");
    }

    #[tokio::test]
    async fn throttles_duplicate_snapshots() {
        use crate::test_helpers::TestSensor;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct CountLLM(Arc<AtomicUsize>);

        #[async_trait::async_trait]
        impl LLMClient for CountLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(futures::stream::empty()))
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
        let mut will = Will::new(llm)
            .prompt("{template}")
            .delay_ms(10)
            .min_llm_interval_ms(50);
        let sensor = TestSensor;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _stream = will.observe_with_abort(vec![sensor], Some(rx)).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = tx.send(());
    }

    #[test]
    fn builder_sets_store() {
        use crate::InMemoryStore;
        let llm = Arc::new(StaticLLM::new(""));
        let store = Arc::new(InMemoryStore::new());
        let will = Will::<serde_json::Value>::new(llm).memory_store(store);
        assert!(will.store.is_some());
    }

    #[tokio::test]
    async fn queries_memory_for_neighbors() {
        use crate::memory_store::{StoredImpression, StoredSensation};
        use chrono::Utc;
        use std::collections::HashMap;

        struct SpyStore {
            queries: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl MemoryStore for SpyStore {
            async fn store_sensation(&self, _s: &StoredSensation) -> anyhow::Result<()> {
                Ok(())
            }
            async fn store_impression(&self, _i: &StoredImpression) -> anyhow::Result<()> {
                Ok(())
            }
            async fn add_lifecycle_stage(
                &self,
                _i: &str,
                _s: &str,
                _d: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn retrieve_related_impressions(
                &self,
                how: &str,
                _k: usize,
            ) -> anyhow::Result<Vec<StoredImpression>> {
                self.queries.lock().unwrap().push(how.to_string());
                Ok(vec![StoredImpression {
                    id: "n".into(),
                    kind: "Instant".into(),
                    when: Utc::now(),
                    how: "memory".into(),
                    sensation_ids: Vec::new(),
                    impression_ids: Vec::new(),
                }])
            }
            async fn fetch_recent_impressions(
                &self,
                _l: usize,
            ) -> anyhow::Result<Vec<StoredImpression>> {
                Ok(Vec::new())
            }
            async fn load_full_impression(
                &self,
                _id: &str,
            ) -> anyhow::Result<(
                StoredImpression,
                Vec<StoredSensation>,
                HashMap<String, String>,
            )> {
                Err(anyhow::anyhow!("not implemented"))
            }
        }

        struct InstantMomentSensor;
        impl Sensor<Impression<Impression<String>>> for InstantMomentSensor {
            fn stream(
                &mut self,
            ) -> BoxStream<'static, Vec<Sensation<Impression<Impression<String>>>>> {
                use async_stream::stream;
                let s = stream! {
                    yield vec![
                        Sensation {
                            kind: "instant".into(),
                            when: chrono::Local::now(),
                            what: Impression { how: "instant".into(), what: Vec::new() },
                            source: None,
                        },
                        Sensation {
                            kind: "moment".into(),
                            when: chrono::Local::now(),
                            what: Impression { how: "moment".into(), what: Vec::new() },
                            source: None,
                        }
                    ];
                };
                Box::pin(s)
            }
        }

        let queries = Arc::new(Mutex::new(Vec::new()));
        let store = Arc::new(SpyStore {
            queries: queries.clone(),
        });
        let llm = Arc::new(StaticLLM::new(""));
        let mut will = Will::new(llm)
            .prompt("{memories}")
            .delay_ms(10)
            .memory_store(store);
        let sensor = InstantMomentSensor;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _stream = will.observe_with_abort(vec![sensor], Some(rx)).await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let _ = tx.send(());
        let q = queries.lock().unwrap();
        assert_eq!(q.len(), 2);
    }

    #[tokio::test]
    async fn waits_for_llm_stream_before_next_loop() {
        use futures::stream::BoxStream;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct RepeatSensor;

        impl Sensor<Impression<String>> for RepeatSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<Impression<String>>>> {
                use async_stream::stream;
                let s = stream! {
                    loop {
                        yield vec![Sensation {
                            kind: "impression".into(),
                            when: chrono::Local::now(),
                            what: Impression { how: "ping".into(), what: Vec::new() },
                            source: None,
                        }];
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                };
                Box::pin(s)
            }
        }

        #[derive(Clone)]
        struct HoldLLM {
            calls: Arc<AtomicUsize>,
            tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Token>>>>,
        }

        #[async_trait::async_trait]
        impl LLMClient for HoldLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                *self.tx.lock().unwrap() = Some(tx);
                Ok(Box::pin(UnboundedReceiverStream::new(rx)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(HoldLLM {
            calls: calls.clone(),
            tx: Arc::new(Mutex::new(None)),
        });
        let mut will = Will::new(llm.clone()).prompt("{template}").delay_ms(5);
        let sensor = RepeatSensor;
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let _stream = will.observe_with_abort(vec![sensor], Some(stop_rx)).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if let Some(tx) = llm.tx.lock().unwrap().take() {
            let _ = tx.send(Token {
                text: "done".into(),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if let Some(tx) = llm.tx.lock().unwrap().take() {
            drop(tx);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(calls.load(Ordering::SeqCst) >= 2);
        let _ = stop_tx.send(());
    }
}
