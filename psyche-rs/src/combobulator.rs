use std::sync::Arc;

use futures::stream::BoxStream;

use crate::{Impression, LLMClient, Sensor, Wit};

/// Second order wit that summarizes impressions from another wit.
///
/// `Combobulator` simply wraps [`Wit`] with an input type of
/// [`Impression`].
pub struct Combobulator<T = serde_json::Value> {
    wit: Wit<Impression<T>>,
}

impl<T> Combobulator<T> {
    /// Creates a new [`Combobulator`] backed by the given LLM client.
    pub fn new(llm: Arc<dyn LLMClient>) -> Self {
        Self { wit: Wit::new(llm) }
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
                when: chrono::Utc::now(),
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
}
