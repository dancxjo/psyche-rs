use std::sync::{Arc, Mutex};

use chrono::Local;
use futures::{StreamExt, stream::BoxStream};
use segtok::segmenter::{SegmentConfig, split_single};
use serde_json::Value;
use std::collections::VecDeque;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace, warn};

use crate::Action;
use crate::conversation::Conversation;
use crate::{
    Intention, Sensation, Sensor,
    llm_client::{LLMClient, LLMTokenStream},
    llm_parser, render_template,
};
use ollama_rs::generation::chat::ChatMessage;

/// Default system prompt template for [`Voice`].
const DEFAULT_PROMPT: &str =
    "Current situation: {situation}\nCurrent instant: {instant}\nRespond as Pete.";

/// LLM-powered conversational reflex.
pub struct Voice {
    llm: Arc<dyn LLMClient>,
    name: String,
    conversation: Arc<Mutex<Conversation>>,
    delay_ms: u64,
    system_prompt: String,
    quick_tx: Option<UnboundedSender<Vec<Sensation<String>>>>,
}

impl Voice {
    /// Create a new [`Voice`] retaining up to `max_tail_len` messages.
    pub fn new(llm: Arc<dyn LLMClient>, max_tail_len: usize) -> Self {
        Self {
            llm,
            name: "Voice".into(),
            conversation: Arc::new(Mutex::new(Conversation::new(max_tail_len))),
            delay_ms: 500,
            system_prompt: DEFAULT_PROMPT.to_string(),
            quick_tx: None,
        }
    }

    /// Set a custom system prompt template.
    pub fn system_prompt(mut self, template: impl Into<String>) -> Self {
        self.system_prompt = template.into();
        self
    }

    /// Adjust the delay between sensor batches.
    pub fn delay_ms(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }

    /// Sets the name used for logging.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Channel used to send planned utterances to Quick.
    pub fn quick_tx(mut self, tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        self.quick_tx = Some(tx);
        self
    }

    /// Observe the provided ear sensor and emit speech intentions.
    pub async fn observe(
        &self,
        mut ear: impl Sensor<String> + Send + 'static,
        get_situation: Arc<dyn Fn() -> String + Send + Sync>,
        get_instant: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> BoxStream<'static, Vec<Intention>> {
        let (tx, rx) = unbounded_channel();
        let llm = self.llm.clone();
        let convo = self.conversation.clone();
        let name = self.name.clone();
        let prompt_tpl = self.system_prompt.clone();
        let delay = self.delay_ms;
        let quick_tx = self.quick_tx.clone();
        tokio::spawn(async move {
            let mut stream = ear.stream();
            let window: Arc<Mutex<Vec<Sensation<serde_json::Value>>>> =
                Arc::new(Mutex::new(Vec::new()));
            let mut pending: VecDeque<String> = VecDeque::new();
            while let Some(batch) = stream.next().await {
                for s in batch {
                    if s.kind == "self_audio" {
                        if let Some(sent) = pending.pop_front() {
                            convo.lock().unwrap().push_assistant(&sent);
                        }
                        continue;
                    }

                    convo.lock().unwrap().push_user(&s.what);
                    let situation = (get_situation)();
                    let instant = (get_instant)();
                    #[derive(serde::Serialize)]
                    struct Ctx<'a> {
                        situation: &'a str,
                        instant: &'a str,
                    }
                    let ctx = Ctx {
                        situation: &situation,
                        instant: &instant,
                    };
                    let system_prompt = render_template(&prompt_tpl, &ctx).unwrap_or_else(|e| {
                        warn!(?e, "voice prompt render failed");
                        prompt_tpl.clone()
                    });
                    trace!(agent=%name, %system_prompt, "voice system prompt");
                    let mut msgs = convo.lock().unwrap().tail();
                    msgs.insert(0, ChatMessage::system(system_prompt));
                    debug!(agent=%name, "Voice LLM call started");
                    match llm.chat_stream(&msgs).await {
                        Ok(mut llm_stream) => {
                            let (tok_tx, tok_rx) = unbounded_channel();
                            let window_clone = window.clone();
                            let tx_clone = tx.clone();
                            let name_clone = name.clone();
                            tokio::spawn(async move {
                                let rx_stream: LLMTokenStream =
                                    Box::pin(UnboundedReceiverStream::new(tok_rx));
                                llm_parser::drive_llm_stream(
                                    &name_clone,
                                    rx_stream,
                                    window_clone,
                                    tx_clone,
                                    None,
                                )
                                .await;
                            });

                            let mut buf = String::new();
                            while let Some(tok) = llm_stream.next().await {
                                match tok {
                                    Ok(t) => {
                                        trace!(agent=%name, %t, "voice llm token");
                                        if !t.contains('<') && !t.contains('>') {
                                            buf.push_str(&t);
                                        }
                                        let _ = tok_tx.send(Ok(t));

                                        let mut sents =
                                            split_single(&buf, SegmentConfig::default());
                                        if let Some(last) = sents.last() {
                                            if !last.trim_end().ends_with(['.', '!', '?']) {
                                                buf = last.clone();
                                                sents.pop();
                                            } else {
                                                buf.clear();
                                            }
                                        }
                                        for sent in sents {
                                            pending.push_back(sent.clone());
                                            let text = sent.clone();
                                            let body_text = text.clone();
                                            let body =
                                                futures::stream::once(async move { body_text })
                                                    .boxed();
                                            let action = Action::new("say", Value::Null, body);
                                            let intent = Intention::to(action).assign("say");
                                            let _ = tx.send(vec![intent]);
                                            if let Some(qtx) = &quick_tx {
                                                let sens = Sensation {
                                                    kind: "utterance.planned".into(),
                                                    when: Local::now(),
                                                    what: format!(
                                                        "I feel myself starting to say: '{text}'"
                                                    ),
                                                    source: None,
                                                };
                                                let _ = qtx.send(vec![sens]);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!(?e, "llm token error");
                                        break;
                                    }
                                }
                            }
                            if !buf.trim().is_empty() {
                                let sent = buf.trim().to_string();
                                pending.push_back(sent.clone());
                                let text = sent.clone();
                                let body_text = text.clone();
                                let body = futures::stream::once(async move { body_text }).boxed();
                                let action = Action::new("say", Value::Null, body);
                                let intent = Intention::to(action).assign("say");
                                let _ = tx.send(vec![intent]);
                                if let Some(qtx) = &quick_tx {
                                    let sens = Sensation {
                                        kind: "utterance.planned".into(),
                                        when: Local::now(),
                                        what: format!("I feel myself starting to say: '{text}'"),
                                        source: None,
                                    };
                                    let _ = qtx.send(vec![sens]);
                                }
                            }
                            drop(tok_tx);
                            debug!(agent=%name, text=%buf, "llm full response");
                            convo.lock().unwrap().push_assistant(&buf);
                        }
                        Err(e) => {
                            warn!(?e, "voice llm failed");
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        });
        UnboundedReceiverStream::new(rx).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::StaticLLM;
    use futures::StreamExt;

    struct TestEar;
    impl Sensor<String> for TestEar {
        fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
            use futures::stream;
            let s = Sensation {
                kind: "utterance.text".into(),
                when: chrono::Local::now(),
                what: "hello".into(),
                source: None,
            };
            stream::once(async move { vec![s] }).boxed()
        }
    }

    #[tokio::test]
    async fn emits_say_intention() {
        let llm = Arc::new(StaticLLM::new("<say>hi</say>"));
        let voice = Voice::new(llm, 5).delay_ms(10);
        let ear = TestEar;
        let get_situation = Arc::new(|| "".to_string());
        let get_instant = Arc::new(|| "".to_string());
        let mut stream = voice.observe(ear, get_situation, get_instant).await;
        let batch = stream.next().await.unwrap();
        assert!(!batch.is_empty());
        assert_eq!(batch[0].assigned_motor, "say");
    }

    #[tokio::test]
    async fn streams_sentences() {
        let llm = Arc::new(StaticLLM::new("Hello there. How are you?"));
        let voice = Voice::new(llm, 5).delay_ms(10);
        let ear = TestEar;
        let get_situation = Arc::new(|| "".to_string());
        let get_instant = Arc::new(|| "".to_string());
        let mut stream = voice.observe(ear, get_situation, get_instant).await;
        let a = stream.next().await.unwrap();
        let b = stream.next().await.unwrap();
        assert_eq!(a[0].assigned_motor, "say");
        assert_eq!(b[0].assigned_motor, "say");
    }
}
