use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, error, trace, warn};

use regex::Regex;

use crate::llm_client::LLMClient;
use crate::{Action, Intention, Motor, PlainDescribe, Sensation, Sensor, render_template};
use ollama_rs::generation::chat::ChatMessage;
use serde_json::{Map, Value};

const DEFAULT_PROMPT: &str = include_str!("prompts/will_prompt.txt");

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

/// Returns `true` if `output` contains any valid XML motor action tag.
/// Tag names are matched using the provided `motors` list and compared
/// case-insensitively.
pub fn contains_motor_action(output: &str, motors: &[MotorDescription]) -> bool {
    if motors.is_empty() {
        return false;
    }
    let names = motors
        .iter()
        .map(|m| regex::escape(&m.name.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("|");
    let re = Regex::new(&format!(r"<({})[^>]*>", names)).expect("valid regex");
    re.is_match(&output.to_ascii_lowercase())
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
    name: String,
    prompt: String,
    delay_ms: u64,
    window_ms: u64,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    motors: Vec<MotorDescription>,
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
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    motors: Vec<MotorDescription>,
    latest_instant_store: Arc<Mutex<String>>,
    latest_moment_store: Arc<Mutex<String>>,
    thoughts_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>>,
    sensors: Vec<S>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Intention>>,
}

impl<T> Will<T> {
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            llm,
            name: "Will".into(),
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

    /// Sets the agent name used for logging.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn prompt(mut self, template: impl Into<String>) -> Self {
        self.prompt = template.into();
        self
    }

    pub fn delay_ms(mut self, delay: u64) -> Self {
        self.delay_ms = delay;
        self
    }

    pub fn window_ms(mut self, ms: u64) -> Self {
        self.window_ms = ms;
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
        self.motors.push(MotorDescription {
            name: name.into(),
            description: description.into(),
        });
        debug!(motor_name = %self.motors.last().unwrap().name, "Will registered motor");
        self
    }

    pub fn register_motor(&mut self, motor: &dyn Motor) -> &mut Self {
        self.motors.push(MotorDescription {
            name: motor.name().to_string(),
            description: motor.description().to_string(),
        });
        debug!(motor_name = %motor.name(), "Will registered motor");
        self
    }

    pub fn timeline(&self) -> String
    where
        T: serde::Serialize + Clone,
    {
        crate::build_timeline(&self.window)
    }

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

        let config = WillRuntimeConfig {
            llm,
            name: self.name.clone(),
            template,
            delay,
            window_ms,
            window,
            motors,
            latest_instant_store,
            latest_moment_store,
            thoughts_tx,
            sensors,
            tx: tx.clone(),
        };
        Self::spawn_runtime(config);

        UnboundedReceiverStream::new(rx).boxed()
    }

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
            window,
            motors,
            latest_instant_store,
            latest_moment_store,
            thoughts_tx,
            sensors,
            tx,
        } = config;

        tokio::spawn(async move {
            debug!(agent = %name, "starting Will thread");
            let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
            let mut sensor_stream = stream::select_all(streams);
            let mut pending: Vec<Sensation<T>> = Vec::new();

            let start_re = Regex::new(r"^<([a-zA-Z0-9_]+)([^>]*)>").unwrap();
            let attr_re = Regex::new(r#"([a-zA-Z0-9_]+)="([^"]*)""#).unwrap();

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

                        let situation = crate::build_timeline(&window);

                        let motor_text = motors
                            .iter()
                            .map(|m| format!("{}: {}", m.name, m.description))
                            .collect::<Vec<_>>()
                            .join("\n");

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

                        #[derive(serde::Serialize)]
                        struct Ctx<'a> {
                            situation: &'a str,
                            motors: &'a str,
                            latest_instant: &'a str,
                            latest_moment: &'a str,
                        }
                        let ctx = Ctx {
                            situation: &situation,
                            motors: &motor_text,
                            latest_instant: &last_instant,
                            latest_moment: &last_moment,
                        };
                        let prompt = render_template(&template, &ctx).unwrap_or_else(|e| {
                            warn!(error=?e, "template render failed");
                            template.clone()
                        });

                        debug!(%prompt, "Will generated prompt");
                        trace!("will invoking llm");

                        let msgs = vec![ChatMessage::user(prompt)];
                        let llm_clone = llm.clone();
                        let tx_clone = tx.clone();
                        let window_clone = window.clone();
                        let thoughts_tx_clone = thoughts_tx.clone();
                        let start_re_clone = start_re.clone();
                        let attr_re_clone = attr_re.clone();
                        let name_clone = name.clone();
                        tokio::spawn(async move {
                            debug!(agent = %name_clone, "LLM call started");
                            match llm_clone.chat_stream(&msgs).await {
                                Ok(mut stream) => {
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
                                                } else if buf.len() > closing.len() {
                                                    let send_len = buf.len() - closing.len();
                                                    let prefix = safe_prefix(&buf, send_len);
                                                    let _ = tx_body.send(prefix.to_string());
                                                    buf.drain(..prefix.len());
                                                    break;
                                                } else if let Some(caps) = start_re_clone.captures(&buf) {
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
                                                            window_clone.lock().unwrap().push(sensation);
                                                        }
                                                        if let Some(tx) = &thoughts_tx_clone {
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
                                                    for cap in attr_re_clone.captures_iter(attrs) {
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
                                                    window_clone.lock().unwrap().push(Sensation {
                                                        kind: "intention".into(),
                                                        when: chrono::Local::now(),
                                                        what,
                                                        source: None,
                                                    });

                                                    let _ = tx_clone.send(vec![intention]);
                                                    state = Some((tag, closing, closing_lower, btx));
                                                } else if let Some(idx) = buf.find('<') {
                                                    let prefix = safe_prefix(&buf, idx);
                                                    pending_text.push_str(prefix);
                                                    buf.drain(..prefix.len());
                                                    break;
                                                } else {
                                                    if !buf.is_empty() {
                                                        pending_text.push_str(&buf);
                                                    }
                                                    buf.clear();
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
                                            window_clone.lock().unwrap().push(sensation);
                                        }
                                        if let Some(tx) = &thoughts_tx_clone {
                                            let s = Sensation {
                                                kind: "thought".into(),
                                                when: chrono::Local::now(),
                                                what: format!("I thought to myself: {}", pending_text.trim()),
                                                source: None,
                                            };
                                            let _ = tx.send(vec![s]);
                                        }
                                    }

                                    debug!(agent = %name_clone, "LLM call ended");
                                    trace!("will llm stream finished");
                                }
                                Err(err) => {
                                    error!(?err, "llm streaming failed");
                                }
                            }
                        });
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_motor_tag() {
        let motors = vec![MotorDescription {
            name: "say".into(),
            description: "".into(),
        }];
        let out = "Thinking <say mood=\"happy\">hi</say>";
        assert!(contains_motor_action(out, &motors));
        assert!(!contains_motor_action("no tag here", &motors));
    }
}
