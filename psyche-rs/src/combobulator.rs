use std::sync::Arc;

use futures::stream::BoxStream;

use crate::{Impression, LLMClient, Sensor, Wit};

/// Default prompt text for [`Combobulator`].
///
/// Narrative prompt text should be supplied by the application using this
/// library. This placeholder keeps the API backwards compatible.
const DEFAULT_PROMPT: &str = "";

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

    /// Sets the agent name used for logging.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.wit = self.wit.name(name);
        self
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
        T: serde::Serialize + Clone,
    {
        self.wit.timeline()
    }

    /// Returns the timeline with a short description prefix.
    pub fn describe_timeline(&self) -> String
    where
        T: serde::Serialize + Clone,
    {
        format!("Situation timeline\n{}", self.timeline())
    }
}

impl<T: Clone> Combobulator<T> {
    /// Mutable variant of [`prompt`].
    pub fn set_prompt(&mut self, template: impl Into<String>) -> &mut Self {
        self.wit = self.wit.clone().prompt(template);
        self
    }

    /// Mutable variant of [`name`].
    pub fn set_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.wit = self.wit.clone().name(name);
        self
    }

    /// Mutable variant of [`delay_ms`].
    pub fn set_delay_ms(&mut self, delay: u64) -> &mut Self {
        self.wit = self.wit.clone().delay_ms(delay);
        self
    }

    /// Mutable variant of [`window_ms`].
    pub fn set_window_ms(&mut self, ms: u64) -> &mut Self {
        self.wit = self.wit.clone().window_ms(ms);
        self
    }
}

impl<T> Combobulator<T>
where
    T: Clone + Send + 'static + serde::Serialize,
{
    /// Observe provided impression sensors and yield summarized impressions.
    ///
    /// Each emitted item is an `Impression` summarizing a batch of lower level
    /// impressions. The resulting stream therefore contains `Vec<Impression<Impression<T>>>`.
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
    ///
    /// Like [`observe`] this returns a stream of summarized impressions where
    /// each summary is an `Impression` over lower level impressions.
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
    use crate::test_helpers::{StaticLLM, TestSensor, TwoBatch};
    use futures::StreamExt;

    #[tokio::test]
    async fn emits_combined_impressions() {
        let llm = Arc::new(StaticLLM {
            reply: "meta".into(),
        });
        let mut comb = Combobulator::new(llm).prompt("{template}").delay_ms(10);
        let sensor = TestSensor;
        let mut stream = comb.observe(vec![sensor]).await;
        let impressions = stream.next().await.unwrap();
        assert_eq!(impressions.len(), 1);
        assert_eq!(impressions[0].how.trim(), "meta");
    }

    #[tokio::test]
    async fn timeline_collects_inputs() {
        let llm = Arc::new(StaticLLM { reply: "ok".into() });
        let mut comb = Combobulator::new(llm).prompt("{template}").delay_ms(10);
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
