use std::sync::{Arc, Mutex};
use std::thread;

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};

use regex::Regex;

use crate::llm_client::LLMClient;
use crate::{Action, Sensation, Sensor};
use ollama_rs::generation::chat::ChatMessage;
use serde_json::{Map, Value};

const DEFAULT_PROMPT: &str = include_str!("will_prompt.txt");

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

    /// Registers a motor description for prompt generation.
    pub fn motor(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.motors.push(MotorDescription {
            name: name.into(),
            description: description.into(),
        });
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

    /// Observe sensors and yield action batches.
    pub async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Action>>
    where
        T: Clone + Send + 'static + serde::Serialize,
        S: Sensor<T> + Send + 'static,
    {
        let (tx, rx) = unbounded_channel();
        let llm = self.llm.clone();
        let template = self.prompt.clone();
        let delay = self.delay_ms;
        let window_ms = self.window_ms;
        let window = self.window.clone();
        let motors = self.motors.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            debug!("will runtime started");
            rt.block_on(async move {
                let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
                let mut sensor_stream = stream::select_all(streams);
                loop {
                    tokio::select! {
                        Some(batch) = sensor_stream.next() => {
                            trace!(count = batch.len(), "sensations received");
                            window.lock().unwrap().extend(batch);
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                            let snapshot = {
                                let mut w = window.lock().unwrap();
                                let cutoff = chrono::Utc::now() - chrono::Duration::milliseconds(window_ms as i64);
                                w.retain(|s| s.when > cutoff);
                                w.clone()
                            };
                            let situation = snapshot.iter().map(|s| {
                                let what = serde_json::to_string(&s.what).unwrap_or_default();
                                format!("{} {} {}", s.when.to_rfc3339(), s.kind, what)
                            }).collect::<Vec<_>>().join("\n");
                            let motor_text = motors.iter().map(|m| format!("{}: {}", m.name, m.description)).collect::<Vec<_>>().join("\n");
                            let prompt = template
                                .replace("{situation}", &situation)
                                .replace("{motors}", &motor_text);
                            trace!(?prompt, "sending will prompt");
                            let msgs = vec![ChatMessage::user(prompt)];
                            match llm.chat_stream(&msgs).await {
                                Ok(mut stream) => {
                                    let start_re = Regex::new(r"^<([a-zA-Z0-9_]+)([^>]*)>").unwrap();
                                    let attr_re = Regex::new(r#"([a-zA-Z0-9_]+)="([^"]*)""#).unwrap();
                                    let mut buf = String::new();
                                    let mut state: Option<(String, String, tokio::sync::mpsc::UnboundedSender<String>)> = None;
                                    while let Some(Ok(tok)) = stream.next().await {
                                        trace!(%tok, "llm token");
                                        buf.push_str(&tok);
                                        loop {
                                            if let Some((ref _name, ref closing, ref tx_body)) = state {
                                                if let Some(pos) = buf.find(closing) {
                                                    if pos > 0 {
                                                        let part = buf[..pos].to_string();
                                                        let _ = tx_body.send(part);
                                                    }
                                                    buf.drain(..pos + closing.len());
                                                    state = None;
                                                    break;
                                                } else {
                                                    if buf.len() > closing.len() {
                                                        let send_len = buf.len() - closing.len();
                                                        let part = buf[..send_len].to_string();
                                                        let _ = tx_body.send(part);
                                                        buf.drain(..send_len);
                                                    }
                                                    break;
                                                }
                                            } else {
                                                if let Some(caps) = start_re.captures(&buf) {
                                                    let tag = caps.get(1).unwrap().as_str().to_string();
                                                    let attrs = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                                                    let mut map = Map::new();
                                                    for cap in attr_re.captures_iter(attrs) {
                                                        map.insert(cap[1].to_string(), Value::String(cap[2].to_string()));
                                                    }
                                                    let closing = format!("</{}>", tag);
                                                    let _ = buf.drain(..caps.get(0).unwrap().end());
                                                    let (btx, brx) = unbounded_channel();
                                                    let action = Action::new(tag.clone(), Value::Object(map), UnboundedReceiverStream::new(brx).boxed());
                                                    let _ = tx.send(vec![action]);
                                                    state = Some((tag, closing, btx));
                                                } else {
                                                    if let Some(idx) = buf.find('<') {
                                                        buf.drain(..idx);
                                                    } else {
                                                        buf.clear();
                                                    }
                                                    break;
                                                }
                                            }
                                        }
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
    struct StaticLLM;

    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
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
                when: chrono::Utc::now(),
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
        let mut actions = stream.next().await.unwrap();
        let mut action = actions.pop().unwrap();
        assert_eq!(action.name, "say");
        let chunks: Vec<String> = action.body.collect().await;
        let body: String = chunks.concat();
        assert_eq!(body, "Hello world");
    }
}
