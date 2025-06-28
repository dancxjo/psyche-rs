//! Core types for the `psyche-rs` crate.
//!
//! This crate currently exposes [`Sensation`], [`Impression`], [`Sensor`] and
//! [`Wit`] building blocks for constructing artificial agents.

mod combobulator;
mod impression;
mod impression_sensor;
mod llm_client;
mod llm_pool;
mod motor;
mod psyche;
mod sensation;
mod sensation_channel_sensor;
mod sensor;
mod will;
mod wit;

pub use crate::llm_client::{LLMClient, OllamaLLM, TokenStream};
pub use combobulator::Combobulator;
pub use impression::Impression;
pub use impression_sensor::ImpressionSensor;
pub use llm_pool::LLMPool;
pub use motor::{
    Action, ActionResult, Completion, Intention, Interruption, Motor, MotorError, Urge,
};
pub use psyche::Psyche;
pub use sensation::Sensation;
pub use sensation_channel_sensor::SensationSensor;
pub use sensor::Sensor;
pub use will::{MotorDescription, Will};
pub use wit::Wit;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use futures::stream::BoxStream;
    use futures::{StreamExt, stream};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn impression_from_sensor() {
        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "test".into(),
                    when: Utc::now(),
                    what: "ping".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        use async_trait::async_trait;
        #[derive(Clone)]
        struct StaticLLM;

        #[async_trait]
        impl LLMClient for StaticLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::pin(stream::once(async {
                    Ok("ping event".to_string())
                })))
            }
        }

        let llm = Arc::new(StaticLLM);
        let mut witness = Wit::new(llm).delay_ms(10);
        let s = TestSensor;
        let mut stream = witness.observe(vec![s]).await;
        if let Some(impressions) = stream.next().await {
            assert_eq!(impressions[0].how, "ping event");
        } else {
            panic!("no impression emitted");
        }
    }

    #[tokio::test]
    async fn round_trip_to_motor() {
        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "utterance.text".into(),
                    when: Utc::now(),
                    what: "ping".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        use async_trait::async_trait;
        #[derive(Clone)]
        struct StaticLLM;

        #[async_trait]
        impl LLMClient for StaticLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::pin(stream::once(async {
                    Ok("ping event".to_string())
                })))
            }
        }

        let llm = Arc::new(StaticLLM);

        struct RecordingMotor {
            log: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl Motor for RecordingMotor {
            fn description(&self) -> &'static str {
                "records actions into a vector"
            }
            fn name(&self) -> &'static str {
                "say"
            }
            async fn perform(&self, mut action: Action) -> Result<ActionResult, MotorError> {
                use futures::StreamExt;
                let mut collected = String::new();
                while let Some(chunk) = action.body.next().await {
                    collected.push_str(&chunk);
                }
                self.log.lock().unwrap().push(collected);
                Ok(ActionResult {
                    sensations: Vec::new(),
                    completed: true,
                })
            }
        }

        let mut witness = Wit::new(llm).delay_ms(10);
        let sensor = TestSensor;

        let log = Arc::new(Mutex::new(Vec::new()));
        let motor = RecordingMotor { log: log.clone() };

        let mut impressions = witness.observe(vec![sensor]).await;

        if let Some(batch) = impressions.next().await {
            for impression in batch {
                let text = impression.how.clone();
                let body = stream::once(async move { text }).boxed();
                let action = Action::new("say", Value::Null, body);
                futures::executor::block_on(async {
                    motor.perform(action).await.unwrap();
                });
            }
        }

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].as_str(), "ping event");
    }
}
