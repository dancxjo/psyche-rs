use std::sync::{Arc, Mutex};
use std::thread;

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, error, trace, warn};

use regex::Regex;

use crate::llm_client::LLMClient;
use crate::{Action, Intention, Motor, Sensation, Sensor};
use ollama_rs::generation::chat::ChatMessage;
use serde_json::{Map, Value};

const DEFAULT_PROMPT: &str = include_str!("prompts/will_prompt.txt");

/// Returns a prefix of `s` that fits within `max_bytes` without splitting UTF-8
/// characters.
///
/// If `max_bytes` does not land on a char boundary, the prefix is truncated to
/// the previous valid boundary and a warning is emitted.
///
/// # Examples
///
/// ```
/// use psyche_rs::safe_prefix;
/// assert_eq!(safe_prefix("aïb", 2), "a");
/// assert_eq!(safe_prefix("abc", 2), "ab");
/// ```
pub fn safe_prefix<'a>(s: &'a str, max_bytes: usize) -> &'a str {
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

/// Description of an available motor.
#[derive(Clone)]
pub struct MotorDescription {
    /// Name of the motor action.
    pub name: String,
    /// Textual description of the motor's capability.
    pub description: String,
}

/// A looping controller that turns sensations into motor actions using an LLM.
pub struct Will<T = serde_json::Value> {
    llm: Arc<dyn LLMClient>,
    prompt: String,
    delay_ms: u64,
    window_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    motors: Vec<MotorDescription>,
    latest_instant: Arc<Mutex<String>>,
    latest_moment: Arc<Mutex<String>>,
    thoughts_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>>,
}

impl<T> Will<T> {
    /// Creates a new [`Will`] backed by the given LLM client.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            prompt: DEFAULT_PROMPT.to_string(),
            delay_ms: 1000,
            window_ms: 60_000,
            window: Arc::new(Mutex::new(Vec::new())),
            motors: Vec::new(),
            latest_instant: Arc::new(Mutex::new(String::new())),
            latest_moment: Arc::new(Mutex::new(String::new())),
            thoughts_tx: None,
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

    /// Sets a channel to emit thought sensations.
    pub fn thoughts(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        self.thoughts_tx = Some(tx);
        self
    }

    /// Registers a motor description for prompt generation.
    pub fn motor(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.motors.push(MotorDescription {
            name: name.into(),
            description: description.into(),
        });
        debug!(motor_name = %self.motors.last().unwrap().name, "Will registered motor");
        self
    }

    /// Registers an existing [`Motor`] using its `name` and `description`.
    pub fn register_motor(&mut self, motor: &dyn Motor) -> &mut Self {
        self.motors.push(MotorDescription {
            name: motor.name().to_string(),
            description: motor.description().to_string(),
        });
        debug!(motor_name = %motor.name(), "Will registered motor");
        self
    }

    /// Returns a textual timeline of sensations in the current window.
    pub fn timeline(&self) -> String
    where
        T: serde::Serialize + Clone,
    {
        let mut sensations = self.window.lock().unwrap().clone();
        sensations.sort_by_key(|s| s.when);
        sensations.dedup_by(|a, b| {
            a.kind == b.kind
                && serde_json::to_string(&a.what).ok() == serde_json::to_string(&b.what).ok()
        });
        sensations
            .iter()
            .map(|s| {
                let what = crate::text_util::to_plain_text(&s.what);
                format!("{} {} {}", s.when.format("%Y-%m-%d %H:%M:%S"), s.kind, what)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Observe sensors and yield intention batches.
    pub async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Intention>>
    where
        T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
        S: Sensor<T> + Send + 'static,
    {
        let (tx, rx) = unbounded_channel();
        let llm = self.llm.clone();
        let template = self.prompt.clone();
        let delay = self.delay_ms;
        let window_ms = self.window_ms;
        let window = self.window.clone();
        let motors = self.motors.clone();
        let latest_instant_store = self.latest_instant.clone();
        let latest_moment_store = self.latest_moment.clone();
        let thoughts_tx = self.thoughts_tx.clone();

        thread::spawn(move || {
            if let Err(err) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                debug!("will runtime started");
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
                            if pending.is_empty() {
                                continue;
                            }
                            trace!("will loop tick");
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
                                trace!("Will skipping LLM call due to empty snapshot");
                                continue;
                            }
                            trace!(snapshot_len = snapshot.len(), "Will captured snapshot");
                            let situation = snapshot
                                .iter()
                                .map(|s| {
                                    let what =
                                        serde_json::to_string(&s.what).unwrap_or_default();
                                    format!(
                                        "{} {} {}",
                                        s.when.format("%Y-%m-%d %H:%M:%S"),
                                        s.kind,
                                        what
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            let motor_text = motors.iter().map(|m| format!("{}: {}", m.name, m.description)).collect::<Vec<_>>().join("\n");
                            let mut last_instant = String::new();
                            let mut last_moment = String::new();
                            for s in &snapshot {
                                let val = serde_json::to_string(&s.what).unwrap_or_default();
                                match s.kind.as_str() {
                                    "instant" => last_instant = val,
                                    "moment" => last_moment = val,
                                    _ => {}
                                }
                            }
                            *latest_instant_store.lock().unwrap() = last_instant.clone();
                            *latest_moment_store.lock().unwrap() = last_moment.clone();
                            let prompt = template
                                .replace("{situation}", &situation)
                                .replace("{motors}", &motor_text)
                                .replace("{latest_instant}", &last_instant)
                                .replace("{latest_moment}", &last_moment);
                            debug!(%prompt, "Will generated prompt");
                            trace!("will invoking llm");
                            let msgs = vec![ChatMessage::user(prompt)];
                            match llm.chat_stream(&msgs).await {
                                Ok(mut stream) => {
                                    let start_re = Regex::new(r"^<([a-zA-Z0-9_]+)([^>]*)>").unwrap();
                                    let attr_re = Regex::new(r#"([a-zA-Z0-9_]+)="([^"]*)""#).unwrap();
                                    let mut buf = String::new();
                                    let mut state: Option<(String, String, String, tokio::sync::mpsc::UnboundedSender<String>)> = None;
                                    let mut pending_text = String::new();
                                    while let Some(Ok(tok)) = stream.next().await {
                                        trace!(token = %tok, "Will received LLM token");
                                        buf.push_str(&tok);
                                        loop {
                                            if let Some((ref _name, ref closing, ref closing_lower, ref tx_body)) = state {
                                                if let Some(pos) = buf.to_ascii_lowercase().find(closing_lower) {
                                                    if pos > 0 {
                                                        let prefix = safe_prefix(&buf, pos);
                                                        let _ = tx_body.send(prefix.to_string());
                                                    }
                                                    let drain_len = safe_prefix(&buf, pos + closing.len()).len();
                                                    buf.drain(..drain_len);
                                                    state = None;
                                                    break;
                                                } else {
                                                    if buf.len() > closing.len() {
                                                        let send_len = buf.len() - closing.len();
                                                        let prefix = safe_prefix(&buf, send_len);
                                                        let _ = tx_body.send(prefix.to_string());
                                                        buf.drain(..prefix.len());
                                                    }
                                                    break;
                                                }
                                            } else {
                                                if let Some(caps) = start_re.captures(&buf) {
                                                    if !pending_text.trim().is_empty() {
                                                        if let Ok(what) = serde_json::from_value::<T>(
                                                            Value::String(pending_text.trim().to_string()),
                                                        ) {
                                                            let sensation = Sensation {
                                                                kind: "thought".into(),
                                                                when: chrono::Local::now(),
                                                                what,
                                                                source: None,
                                                            };
                                                            window.lock().unwrap().push(sensation);
                                                        }
                                                        if let Some(tx) = &thoughts_tx {
                                                            let s = Sensation {
                                                                kind: "thought".into(),
                                                                when: chrono::Local::now(),
                                                                what: format!("I thought to myself: {}", pending_text.trim()),
                                                                source: None,
                                                            };
                                                            let _ = tx.send(vec![s]);
                                                        }
                                                        pending_text.clear();
                                                    }
                                                    let tag = caps.get(1).unwrap().as_str().to_ascii_lowercase();
                                                    let attrs = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                                                    let mut map = Map::new();
                                                    for cap in attr_re.captures_iter(attrs) {
                                                        map.insert(cap[1].to_string(), Value::String(cap[2].to_string()));
                                                    }
                                                    let closing = format!("</{}>", tag);
                                                    let closing_lower = closing.to_ascii_lowercase();
                                                    let _ = buf.drain(..caps.get(0).unwrap().end());
                                                    let (btx, brx) = unbounded_channel();
                                                    let action = Action::new(tag.clone(), Value::Object(map.clone()), UnboundedReceiverStream::new(brx).boxed());
                                                    let intention = Intention::to(action).assign(tag.clone());
                                                    debug!(motor_name = %tag, "Will assigned motor on intention");
                                                    debug!(?intention, "Will built intention");
                                                    let val = serde_json::to_value(&intention).unwrap();
                                                    let what = serde_json::from_value(val).unwrap_or_default();
                                                    window.lock().unwrap().push(Sensation {
                                                        kind: "intention".into(),
                                                        when: chrono::Local::now(),
                                                        what,
                                                        source: None,
                                                    });
                                                    let _ = tx.send(vec![intention]);
                                                    state = Some((tag, closing, closing_lower, btx));
                                                } else {
                                                    if let Some(idx) = buf.find('<') {
                                                        let prefix = safe_prefix(&buf, idx);
                                                        pending_text.push_str(prefix);
                                                        buf.drain(..prefix.len());
                                                    } else {
                                                        if !buf.is_empty() {
                                                            pending_text.push_str(&buf);
                                                        }
                                                        buf.clear();
                                                    }
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if !pending_text.trim().is_empty() {
                                        if let Ok(what) = serde_json::from_value::<T>(
                                            Value::String(pending_text.trim().to_string()),
                                        ) {
                                            let sensation = Sensation {
                                                kind: "thought".into(),
                                                when: chrono::Local::now(),
                                                what,
                                                source: None,
                                            };
                                            window.lock().unwrap().push(sensation);
                                        }
                                        if let Some(tx) = &thoughts_tx {
                                            let s = Sensation {
                                                kind: "thought".into(),
                                                when: chrono::Local::now(),
                                                what: format!("I thought to myself: {}", pending_text.trim()),
                                                source: None,
                                            };
                                            let _ = tx.send(vec![s]);
                                        }
                                    }
                                    trace!("will llm stream finished");
                                }
                                Err(err) => {
                                    trace!(?err, "llm streaming failed");
                                }
                            }
                        }
                    }
                }
            });
            })) {
                error!(?err, "will runtime crashed");
            }
        });

        UnboundedReceiverStream::new(rx).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_client::{LLMClient, LLMTokenStream};
    use crate::{ActionResult, MotorError};
    use async_trait::async_trait;
    use futures::{StreamExt, stream};

    #[derive(Clone)]
    struct StaticLLM;

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
            let tokens = vec![
                "<say mood=\"calm\">".to_string(),
                "Hello ".to_string(),
                "world".to_string(),
                "</say>".to_string(),
            ];
            Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
        }
    }

    struct DummySensor;

    impl Sensor<String> for DummySensor {
        fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
            let s = Sensation {
                kind: "test".into(),
                when: chrono::Local::now(),
                what: "foo".into(),
                source: None,
            };
            stream::once(async move { vec![s] }).boxed()
        }
    }

    #[tokio::test]
    async fn streams_actions() {
        let llm = Arc::new(StaticLLM);
        let mut will = Will::new(llm).delay_ms(10);
        will = will.motor("say", "speak via speakers");
        let sensor = DummySensor;
        let mut stream = will.observe(vec![sensor]).await;
        let mut intentions = stream.next().await.unwrap();
        let intention = intentions.pop().unwrap();
        assert_eq!(intention.action.name, "say");
        let chunks: Vec<String> = intention.action.body.collect().await;
        let body: String = chunks.concat();
        assert_eq!(body, "Hello world");
    }

    #[tokio::test]
    async fn streams_actions_case_insensitive() {
        #[derive(Clone)]
        struct StaticUpperLLM;

        #[async_trait]
        impl LLMClient for StaticUpperLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let tokens = vec!["<Say>".to_string(), "Hi".to_string(), "</Say>".to_string()];
                Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
            }
        }

        let llm = Arc::new(StaticUpperLLM);
        let mut will = Will::new(llm).delay_ms(10).motor("say", "speak");
        let sensor = DummySensor;
        let mut stream = will.observe(vec![sensor]).await;
        let mut intentions = stream.next().await.unwrap();
        let intention = intentions.pop().unwrap();
        assert_eq!(intention.action.name, "say");
        let collected: Vec<String> = intention.action.body.collect().await;
        assert_eq!(collected.concat(), "Hi");
    }

    #[tokio::test]
    async fn prompt_includes_latest() {
        #[derive(Clone)]
        struct RecLLM {
            prompts: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl LLMClient for RecLLM {
            async fn chat_stream(
                &self,
                msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.prompts.lock().unwrap().push(msgs[0].content.clone());
                Ok(Box::pin(stream::once(async { Ok("<say></say>".into()) })))
            }
        }

        struct InstantSensor;

        impl Sensor<String> for InstantSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "instant".into(),
                    when: chrono::Local::now(),
                    what: "flash".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let llm = Arc::new(RecLLM {
            prompts: prompts.clone(),
        });
        let mut will = Will::new(llm)
            .delay_ms(10)
            .prompt("{latest_instant}-{latest_moment}");
        will = will.motor("say", "speak");
        let sensor = InstantSensor;
        let mut stream = will.observe(vec![sensor]).await;
        let _ = stream.next().await;
        let data = prompts.lock().unwrap();
        assert!(!data.is_empty());
        assert!(data[0].contains("flash"));
    }

    #[tokio::test]
    async fn prompt_includes_motor_descriptions() {
        #[derive(Clone)]
        struct RecLLM {
            prompts: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl LLMClient for RecLLM {
            async fn chat_stream(
                &self,
                msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.prompts.lock().unwrap().push(msgs[0].content.clone());
                Ok(Box::pin(stream::once(async {
                    Ok("<dummy></dummy>".into())
                })))
            }
        }

        struct InstantSensor;

        impl Sensor<String> for InstantSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "instant".into(),
                    when: chrono::Local::now(),
                    what: "flash".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        struct DummyMotor;

        #[async_trait]
        impl Motor for DummyMotor {
            fn description(&self) -> &'static str {
                "dummy motor"
            }
            fn name(&self) -> &'static str {
                "dummy"
            }
            async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
                Ok(ActionResult::default())
            }
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let llm = Arc::new(RecLLM {
            prompts: prompts.clone(),
        });
        let mut will = Will::new(llm).delay_ms(10);
        let motor = DummyMotor;
        will.register_motor(&motor);
        let sensor = InstantSensor;
        let mut stream = will.observe(vec![sensor]).await;
        let _ = stream.next().await;
        let data = prompts.lock().unwrap();
        assert!(!data.is_empty());
        assert!(data[0].contains("dummy: dummy motor"));
    }

    #[tokio::test]
    async fn avoids_duplicates_without_input() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct CountLLM(Arc<AtomicUsize>);

        #[async_trait]
        impl LLMClient for CountLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(stream::once(async { Ok("<a></a>".to_string()) })))
            }
        }

        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "t".into(),
                    when: chrono::Local::now(),
                    what: "hi".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(CountLLM(calls.clone()));
        let mut will = Will::new(llm).delay_ms(10).motor("a", "do A");
        let sensor = TestSensor;
        let mut stream = will.observe(vec![sensor]).await;
        let _ = stream.next().await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn waits_for_llm_before_next_tick() {
        use async_stream::stream;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct SlowLLM(Arc<AtomicUsize>);

        #[async_trait]
        impl LLMClient for SlowLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                let s = stream! {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    yield Ok("<a>".to_string());
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    yield Ok("</a>".to_string());
                };
                Ok(Box::pin(s))
            }
        }

        struct SlowSensor;

        impl Sensor<String> for SlowSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = stream! {
                    loop {
                        yield vec![Sensation {
                            kind: "t".into(),
                            when: chrono::Local::now(),
                            what: "hi".into(),
                            source: None,
                        }];
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                };
                Box::pin(s)
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(SlowLLM(calls.clone()));
        let mut will = Will::new(llm).delay_ms(10).motor("a", "do A");
        let sensor = SlowSensor;
        let mut stream = will.observe(vec![sensor]).await;
        let _ = stream.next().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn handles_multibyte_tokens() {
        #[derive(Clone)]
        struct UnicodeLLM;

        #[async_trait]
        impl LLMClient for UnicodeLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let tokens = vec![
                    "<say>".to_string(),
                    "as \u{201c}".to_string(),
                    "Pete".to_string(),
                    "</say>".to_string(),
                ];
                Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
            }
        }

        let llm = Arc::new(UnicodeLLM);
        let mut will = Will::new(llm).delay_ms(10).motor("say", "speak");
        let sensor = DummySensor;
        let mut stream = will.observe(vec![sensor]).await;
        let mut intentions = stream.next().await.unwrap();
        let intention = intentions.pop().unwrap();
        let chunks: Vec<String> = intention.action.body.collect().await;
        assert_eq!(chunks.concat(), "as \u{201c}Pete");
    }
}
