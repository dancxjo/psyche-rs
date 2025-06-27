use std::sync::Arc;
use std::thread;

use async_trait::async_trait;
use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

use segtok::segmenter::{SegmentConfig, split_single};

use crate::{Impression, Sensation, Sensor, Witness};

use llm::LLMProvider;
use llm::chat::ChatMessage;

/// Default prompt text for [`Wit`].
const DEFAULT_PROMPT: &str = include_str!("wit_prompt.txt");

/// A looping prompt witness that summarizes sensed experiences using an LLM.
pub struct Wit<T = serde_json::Value> {
    llm: Arc<dyn LLMProvider>,
    prompt: String,
    delay_ms: u64,
    min_queue: usize,
    max_queue: usize,
    window: Vec<Sensation<T>>,
}

impl<T> Wit<T> {
    /// Creates a new [`Wit`] with default configuration.
    pub fn new(llm: Arc<dyn LLMProvider>) -> Self {
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
            rt.block_on(async move {
                let streams: Vec<_> = sensors.into_iter().map(|mut s| s.stream()).collect();
                let mut sensor_stream = stream::select_all(streams);
                let mut pending: Vec<Sensation<T>> = Vec::new();
                loop {
                    tokio::select! {
                        Some(batch) = sensor_stream.next() => {
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
                            let prompt = template.replace("{template}", &timeline);
                            let msgs = vec![ChatMessage::user().content(prompt).build()];
                            if let Ok(mut stream) = llm.chat_stream(&msgs).await {
                                let mut text = String::new();
                                while let Some(Ok(tok)) = stream.next().await {
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
                                    let _ = tx.send(impressions);
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
    use async_trait::async_trait;
    use futures::StreamExt;
    use llm::chat::ChatProvider;

    #[derive(Clone)]
    struct StaticLLM {
        reply: String,
    }

    #[derive(Debug)]
    struct SimpleResponse(String);

    impl std::fmt::Display for SimpleResponse {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl llm::chat::ChatResponse for SimpleResponse {
        fn text(&self) -> Option<String> {
            Some(self.0.clone())
        }

        fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
            None
        }
    }

    #[async_trait]
    impl ChatProvider for StaticLLM {
        async fn chat_with_tools(
            &self,
            _msgs: &[ChatMessage],
            _tools: Option<&[llm::chat::Tool]>,
        ) -> Result<Box<dyn llm::chat::ChatResponse>, llm::error::LLMError> {
            Ok(Box::new(SimpleResponse(self.reply.clone())))
        }

        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<
            std::pin::Pin<
                Box<dyn futures::Stream<Item = Result<String, llm::error::LLMError>> + Send>,
            >,
            llm::error::LLMError,
        > {
            let words: Vec<String> = self
                .reply
                .split_whitespace()
                .map(|w| format!("{} ", w))
                .collect();
            let stream = stream::iter(words.into_iter().map(Ok));
            Ok(Box::pin(stream))
        }
    }

    #[async_trait]
    impl llm::completion::CompletionProvider for StaticLLM {
        async fn complete(
            &self,
            _req: &llm::completion::CompletionRequest,
        ) -> Result<llm::completion::CompletionResponse, llm::error::LLMError> {
            Ok(llm::completion::CompletionResponse {
                text: self.reply.clone(),
            })
        }
    }

    #[async_trait]
    impl llm::embedding::EmbeddingProvider for StaticLLM {
        async fn embed(&self, _text: Vec<String>) -> Result<Vec<Vec<f32>>, llm::error::LLMError> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl llm::stt::SpeechToTextProvider for StaticLLM {
        async fn transcribe(&self, _audio: Vec<u8>) -> Result<String, llm::error::LLMError> {
            Ok(String::new())
        }
    }

    #[async_trait]
    impl llm::tts::TextToSpeechProvider for StaticLLM {
        async fn speech(&self, _text: &str) -> Result<Vec<u8>, llm::error::LLMError> {
            Ok(vec![])
        }
    }

    impl LLMProvider for StaticLLM {}

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
