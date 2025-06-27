//! Core types for the `psyche-rs` crate.
//!
//! This crate currently exposes [`Sensation`], [`Impression`], [`Sensor`] and
//! [`Witness`] building blocks for constructing artificial agents.

mod impression;
mod motor;
mod sensation;
mod sensor;
mod wit;
mod witness;

pub use impression::Impression;
pub use motor::{Motor, MotorCommand, MotorExecutor};
pub use sensation::Sensation;
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
    use std::collections::HashMap;
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
            log: Arc<Mutex<Vec<MotorCommand<String>>>>,
        }

        #[async_trait(?Send)]
        impl Motor<String> for RecordingMotor {
            async fn execute(&mut self, command: MotorCommand<String>) {
                self.log.lock().unwrap().push(command);
            }
        }

        struct SimpleMotorExecutor {
            motors: HashMap<String, Box<dyn Motor<String>>>,
        }

        impl SimpleMotorExecutor {
            fn new() -> Self {
                Self {
                    motors: HashMap::new(),
                }
            }

            fn register_motor(&mut self, name: &str, motor: Box<dyn Motor<String>>) {
                self.motors.insert(name.into(), motor);
            }
        }

        #[async_trait(?Send)]
        impl MotorExecutor<String> for SimpleMotorExecutor {
            async fn submit(&mut self, command: MotorCommand<String>) {
                if let Some(motor) = self.motors.get_mut(&command.name) {
                    motor.execute(command).await;
                }
            }
        }

        let mut witness = TestWitness;
        let sensor = TestSensor;

        let log = Arc::new(Mutex::new(Vec::new()));
        let motor = RecordingMotor { log: log.clone() };

        let mut executor = SimpleMotorExecutor::new();
        executor.register_motor("say", Box::new(motor));

        let mut impressions = witness.observe(vec![sensor]).await;

        if let Some(batch) = impressions.next().await {
            for impression in batch {
                let cmd = MotorCommand::<String> {
                    name: "say".into(),
                    args: "meta".to_string(),
                    content: Some(impression.how.clone()),
                };
                executor.submit(cmd).await;
            }
        }

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].content.as_deref(), Some("ping event"));
    }
}
