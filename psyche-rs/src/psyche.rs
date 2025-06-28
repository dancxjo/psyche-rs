use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, info};

use crate::{Action, ActionResult, Intention, Motor, MotorError, Sensation, Sensor, Urge, Wit};

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
    motors: std::collections::HashMap<String, Box<dyn Motor + Send>>,
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
            motors: std::collections::HashMap::new(),
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
        let name = motor.name().to_string();
        self.motors.insert(name, Box::new(motor));
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
                        for motor in self.motors.values() {
                            use futures::stream;
                            let t = text.clone();
                            let body = stream::once(async move { t }).boxed();
                            let urge = Urge::new("log");
                            let intention = Intention::new(urge, motor.name());
                            let action = Action::from_intention(intention, body);
                            if let Err(e) = motor.perform(action).await {
                                debug!(?e, "motor action failed");
                            }
                        }
                    }
                }
            }
        }
    }

    /// Process a single [`Urge`] by dispatching it to the matching motor.
    pub async fn process_urge(&self, urge: Urge) -> Result<ActionResult, MotorError> {
        use futures::stream;
        tracing::trace!(?urge, "processing urge");
        let motor = self
            .motors
            .get(&urge.name)
            .ok_or(MotorError::Unrecognized)?;
        let body = match urge.body.clone() {
            Some(b) => stream::once(async move { b }).boxed(),
            None => stream::empty().boxed(),
        };
        let intention = Intention::new(urge.clone(), motor.name());
        tracing::trace!(?intention, "created intention");
        let action = Action::from_intention(intention.clone(), body);
        tracing::trace!(?action, "dispatching action");
        let res = motor.perform(action).await;
        match &res {
            Ok(r) => tracing::trace!(?r, "action completed"),
            Err(e) => tracing::trace!(?e, "action failed"),
        }
        res
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
    #[async_trait::async_trait]
    impl Motor for CountMotor {
        fn name(&self) -> &'static str {
            "count"
        }
        fn description(&self) -> &'static str {
            "counts how many actions were performed"
        }
        async fn perform(&self, _action: Action) -> Result<ActionResult, MotorError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ActionResult {
                sensations: Vec::new(),
                completed: true,
            })
        }
    }

    #[tokio::test]
    async fn psyche_runs() {
        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(StaticLLM);
        let wit = Wit::new(llm).delay_ms(10);
        let psyche = Psyche::<String>::new()
            .sensor(TestSensor)
            .wit(wit)
            .motor(CountMotor(count.clone()));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn process_urge_returns_sensations() {
        struct EchoMotor {
            name: &'static str,
            kind: &'static str,
        }
        #[async_trait::async_trait]
        impl Motor for EchoMotor {
            fn name(&self) -> &'static str {
                self.name
            }
            fn description(&self) -> &'static str {
                "echo"
            }
            async fn perform(&self, _action: Action) -> Result<ActionResult, MotorError> {
                Ok(ActionResult {
                    sensations: vec![Sensation {
                        kind: self.kind.into(),
                        when: chrono::Utc::now(),
                        what: serde_json::Value::Null,
                        source: None,
                    }],
                    completed: true,
                })
            }
        }

        let psyche = Psyche::<serde_json::Value>::new()
            .motor(EchoMotor {
                name: "look",
                kind: "image/jpeg",
            })
            .motor(EchoMotor {
                name: "listen",
                kind: "audio/wav",
            })
            .motor(EchoMotor {
                name: "sniff",
                kind: "chemical/smell",
            });

        for (name, kind) in [
            ("look", "image/jpeg"),
            ("listen", "audio/wav"),
            ("sniff", "chemical/smell"),
        ] {
            let urge = Urge::new(name);
            let res = psyche.process_urge(urge).await.unwrap();
            assert_eq!(res.sensations[0].kind, kind);
            assert!(res.completed);
        }
    }
}
