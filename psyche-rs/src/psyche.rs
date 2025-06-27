use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, info};

#[cfg(test)]
use crate::MotorError;
use crate::{Action, Impression, Motor, Sensation, Sensor, Wit};
use serde_json::Value;

/// Sensor wrapper enabling shared ownership.
struct SharedSensor<T> {
    inner: Arc<Mutex<dyn Sensor<T> + Send>>,
}

impl<T> Clone for SharedSensor<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> SharedSensor<T> {
    fn new(inner: Arc<Mutex<dyn Sensor<T> + Send>>) -> Self {
        Self { inner }
    }
}

impl<T> Sensor<T> for SharedSensor<T> {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<T>>> {
        let mut sensor = self.inner.lock().expect("lock");
        sensor.stream()
    }
}

/// Core orchestrator coordinating sensors, wits and motors.
pub struct Psyche<T = serde_json::Value> {
    sensors: Vec<Arc<Mutex<dyn Sensor<T> + Send>>>,
    motors: Vec<Box<dyn Motor + Send>>,
    wits: Vec<Wit<T>>,
}

impl<T> Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize,
{
    /// Create an empty [`Psyche`].
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
            motors: Vec::new(),
            wits: Vec::new(),
        }
    }

    /// Add a sensor to the psyche.
    pub fn sensor(mut self, sensor: impl Sensor<T> + Send + 'static) -> Self {
        self.sensors.push(Arc::new(Mutex::new(sensor)));
        self
    }

    /// Add a motor to the psyche.
    pub fn motor(mut self, motor: impl Motor + Send + 'static) -> Self {
        self.motors.push(Box::new(motor));
        self
    }

    /// Add a wit to the psyche.
    pub fn wit(mut self, wit: Wit<T>) -> Self {
        self.wits.push(wit);
        self
    }
}

impl<T> Default for Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize,
{
    /// Run the psyche until interrupted.
    pub async fn run(mut self) {
        debug!("starting psyche");
        let mut streams = Vec::new();
        for wit in self.wits.iter_mut() {
            let sensors_for_wit: Vec<_> = self
                .sensors
                .iter()
                .map(|s| SharedSensor::new(s.clone()))
                .collect();
            let stream = wit.observe(sensors_for_wit).await;
            streams.push(stream);
        }
        let mut merged = stream::select_all(streams);
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("psyche shutting down");
                    break;
                }
                Some(batch) = merged.next() => {
                    for impression in batch {
                        debug!(?impression.how, "impression received");
                        let text = impression.how.clone();
                        for motor in self.motors.iter() {
                            use futures::stream;
                            let t = text.clone();
                            let body = stream::once(async move { t }).boxed();
                            let action = Action::new("log", Value::Null, body);
                            if let Err(e) = motor.perform(action) {
                                debug!(?e, "motor action failed");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LLMClient, TokenStream};
    use futures::{StreamExt, stream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestSensor;
    impl Sensor<String> for TestSensor {
        fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
            let s = Sensation {
                kind: "t".into(),
                when: chrono::Utc::now(),
                what: "hi".into(),
                source: None,
            };
            stream::once(async move { vec![s] }).boxed()
        }
    }

    #[derive(Clone)]
    struct StaticLLM;

    #[async_trait::async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ollama_rs::generation::chat::ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Box::pin(stream::once(async { Ok("hi".to_string()) })))
        }
    }

    struct CountMotor(Arc<AtomicUsize>);
    impl Motor for CountMotor {
        fn description(&self) -> &'static str {
            "counts how many actions were performed"
        }
        fn perform(&self, _action: Action) -> Result<(), MotorError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn psyche_runs() {
        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(StaticLLM);
        let wit = Wit::new(llm).delay_ms(10);
        let psyche = Psyche::new()
            .sensor(TestSensor)
            .wit(wit)
            .motor(CountMotor(count.clone()));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) > 0);
    }
}
