use std::sync::{Arc, Mutex};

use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace, warn};

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
        tokio::spawn(async move {
            let mut stream = ear.stream();
            let window: Arc<Mutex<Vec<Sensation<serde_json::Value>>>> =
                Arc::new(Mutex::new(Vec::new()));
            while let Some(batch) = stream.next().await {
                for s in batch {
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

                            let mut reply = String::new();
                            while let Some(tok) = llm_stream.next().await {
                                match tok {
                                    Ok(t) => {
                                        trace!(agent=%name, %t, "voice llm token");
                                        reply.push_str(&t);
                                        let _ = tok_tx.send(Ok(t));
                                    }
                                    Err(e) => {
                                        warn!(?e, "llm token error");
                                        break;
                                    }
                                }
                            }
                            drop(tok_tx);
                            debug!(agent=%name, %reply, "llm full response");
                            convo.lock().unwrap().push_assistant(&reply);
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
}
