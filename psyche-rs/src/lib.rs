//! Core types for the `psyche-rs` crate.
//!
//! This crate currently exposes [`Sensation`], [`Impression`], [`Sensor`] and
//! [`Witness`] building blocks for constructing artificial agents.

mod combobulator;
mod impression;
mod impression_sensor;
mod llm_client;
mod motor;
mod psyche;
mod sensation;
mod sensation_channel_sensor;
mod sensor;
mod wit;
mod witness;

pub use crate::llm_client::{LLMClient, OllamaLLM};
pub use combobulator::Combobulator;
pub use impression::Impression;
pub use impression_sensor::ImpressionSensor;
pub use motor::{Action, Completion, Intention, Interruption, Motor, MotorError, Urge};
pub use psyche::Psyche;
pub use sensation::Sensation;
pub use sensation_channel_sensor::SensationSensor;
pub use sensor::Sensor;
pub use wit::Wit;
pub use witness::Witness;

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

        struct TestWitness;

        #[async_trait(?Send)]
        impl Witness<String> for TestWitness {
            async fn observe<S>(
                &mut self,
                mut sensors: Vec<S>,
            ) -> BoxStream<'static, Vec<Impression<String>>>
            where
                S: Sensor<String> + 'static,
            {
                let impressions = sensors.pop().unwrap().stream().map(|what| {
                    vec![Impression {
                        how: format!("{} event", what[0].what),
                        what,
                    }]
                });
                impressions.boxed()
            }
        }

        let mut witness = TestWitness;
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

        struct TestWitness;

        #[async_trait(?Send)]
        impl Witness<String> for TestWitness {
            async fn observe<S>(
                &mut self,
                mut sensors: Vec<S>,
            ) -> BoxStream<'static, Vec<Impression<String>>>
            where
                S: Sensor<String> + 'static,
            {
                sensors
                    .pop()
                    .unwrap()
                    .stream()
                    .map(|what| {
                        vec![Impression {
                            how: format!("{} event", what[0].what),
                            what,
                        }]
                    })
                    .boxed()
            }
        }

        struct RecordingMotor {
            log: Arc<Mutex<Vec<String>>>,
        }

        impl Motor for RecordingMotor {
            fn perform(&self, mut action: Action) -> Result<(), MotorError> {
                use futures::StreamExt;
                use futures::executor::block_on;
                let log = &self.log;
                block_on(async {
                    let mut collected = String::new();
                    while let Some(chunk) = action.body.next().await {
                        collected.push_str(&chunk);
                    }
                    log.lock().unwrap().push(collected);
                });
                Ok(())
            }
        }

        let mut witness = TestWitness;
        let sensor = TestSensor;

        let log = Arc::new(Mutex::new(Vec::new()));
        let motor = RecordingMotor { log: log.clone() };

        let mut impressions = witness.observe(vec![sensor]).await;

        if let Some(batch) = impressions.next().await {
            for impression in batch {
                let text = impression.how.clone();
                let body = stream::once(async move { text }).boxed();
                let action = Action::new("say", Value::Null, body);
                motor.perform(action).unwrap();
            }
        }

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].as_str(), "ping event");
    }
}
