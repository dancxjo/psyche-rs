use std::sync::Arc;

use futures::stream::BoxStream;

use crate::{Impression, LLMClient, Sensor, Wit};

/// Default prompt text for [`Combobulator`].
const DEFAULT_PROMPT: &str = include_str!("combobulator_prompt.txt");

/// Second order wit that summarizes impressions from another wit.
///
/// `Combobulator` simply wraps [`Wit`] with an input type of
/// [`Impression`].
///
/// # Example
/// ```ignore
/// let comb = Combobulator::new(llm).prompt("my prompt").delay_ms(1000);
/// let stream = comb.observe(sensors).await;
/// ```
pub struct Combobulator<T = serde_json::Value> {
    wit: Wit<Impression<T>>,
}

impl<T> Combobulator<T> {
    /// Returns the default prompt text.
    pub fn default_prompt() -> &'static str {
        DEFAULT_PROMPT
    }

    /// Creates a new [`Combobulator`] backed by the given LLM client.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self {
            wit: Wit::new(llm).prompt(Self::default_prompt()),
        }
    }

    /// Overrides the prompt template.
    pub fn prompt(mut self, template: impl Into<String>) -> Self {
        self.wit = self.wit.prompt(template);
        self
    }

    /// Sets the sleep delay between ticks.
    pub fn delay_ms(mut self, delay: u64) -> Self {
        self.wit = self.wit.delay_ms(delay);
        self
    }

    /// Sets the sensation window duration in milliseconds.
    pub fn window_ms(mut self, ms: u64) -> Self {
        self.wit = self.wit.window_ms(ms);
        self
    }

    /// Returns a textual timeline of sensations in the current window.
    pub fn timeline(&self) -> String
    where
        T: serde::Serialize,
    {
        self.wit.timeline()
    }
}

impl<T> Combobulator<T>
where
    T: Clone + Send + 'static + serde::Serialize,
{
    /// Observe provided impression sensors and yield summarized impressions.
    pub async fn observe<S>(
        &mut self,
        sensors: Vec<S>,
    ) -> BoxStream<'static, Vec<Impression<Impression<T>>>>
    where
        S: Sensor<Impression<T>> + Send + 'static,
    {
        self.wit.observe(sensors).await
    }

    /// Observe sensors with the ability to abort processing.
    pub async fn observe_with_abort<S>(
        &mut self,
        sensors: Vec<S>,
        abort: tokio::sync::oneshot::Receiver<()>,
    ) -> BoxStream<'static, Vec<Impression<Impression<T>>>>
    where
        S: Sensor<Impression<T>> + Send + 'static,
    {
        self.wit.observe_with_abort(sensors, abort).await
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
            _msgs: &[ollama_rs::generation::chat::ChatMessage],
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

    impl Sensor<Impression<String>> for TestSensor {
        fn stream(&mut self) -> BoxStream<'static, Vec<crate::Sensation<Impression<String>>>> {
            let imp = Impression {
                how: "hello".into(),
                what: Vec::new(),
            };
            let s = crate::Sensation {
                kind: "impression".into(),
                when: chrono::Local::now(),
                what: imp,
                source: None,
            };
            stream::once(async move { vec![s] }).boxed()
        }
    }

    #[tokio::test]
    async fn emits_combined_impressions() {
        let llm = Arc::new(StaticLLM {
            reply: "meta".into(),
        });
        let mut comb = Combobulator::new(llm).delay_ms(10);
        let sensor = TestSensor;
        let mut stream = comb.observe(vec![sensor]).await;
        let impressions = stream.next().await.unwrap();
        assert_eq!(impressions.len(), 1);
        assert_eq!(impressions[0].how.trim(), "meta");
    }

    #[tokio::test]
    async fn timeline_collects_inputs() {
        use async_stream::stream;
        struct TwoBatch;

        impl Sensor<Impression<String>> for TwoBatch {
            fn stream(&mut self) -> BoxStream<'static, Vec<crate::Sensation<Impression<String>>>> {
                let s = stream! {
                    yield vec![crate::Sensation {
                        kind: "impression".into(),
                        when: chrono::Local::now(),
                        what: Impression { how: "a".into(), what: Vec::new() },
                        source: None,
                    }];
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    yield vec![crate::Sensation {
                        kind: "impression".into(),
                        when: chrono::Local::now(),
                        what: Impression { how: "b".into(), what: Vec::new() },
                        source: None,
                    }];
                };
                Box::pin(s)
            }
        }

        let llm = Arc::new(StaticLLM { reply: "ok".into() });
        let mut comb = Combobulator::new(llm).delay_ms(10);
        let sensor = TwoBatch;
        let mut stream = comb.observe(vec![sensor]).await;
        let _ = stream.next().await;
        let _ = stream.next().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let tl = comb.timeline();
        let lines: Vec<_> = tl.lines().collect();
        assert_eq!(lines.len(), 2);
    }
}
