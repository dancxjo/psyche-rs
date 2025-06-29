use std::sync::{Arc, Mutex};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, info};

use crate::{
    Action, ActionResult, Intention, Motor, MotorError, Sensation, Sensor, Urge, Will, Wit,
};
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
///
/// ```no_run
/// use psyche_rs::{Psyche, Wit, Sensor, LLMClient, TokenStream, Sensation};
/// use async_trait::async_trait;
/// use futures::{stream, StreamExt};
/// use std::sync::Arc;
///
/// #[derive(Clone)]
/// struct DummyLLM;
/// #[async_trait]
/// impl LLMClient for DummyLLM {
///     async fn chat_stream(
///         &self,
///         _msgs: &[ollama_rs::generation::chat::ChatMessage],
///     ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(Box::pin(stream::empty()))
///     }
/// }
/// struct DummySensor;
/// impl Sensor<String> for DummySensor {
///     fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
///         stream::empty().boxed()
///     }
/// }
/// let llm = Arc::new(DummyLLM);
/// let wit = Wit::new(llm);
/// let _psyche = Psyche::new().sensor(DummySensor).wit(wit);
/// ```
pub struct Psyche<T = serde_json::Value> {
    sensors: Vec<Arc<Mutex<dyn Sensor<T> + Send>>>,
    motors: Vec<Box<dyn Motor + Send>>,
    wits: Vec<Wit<T>>,
    wills: Vec<Will<T>>,
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
            wills: Vec::new(),
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

    /// Add a will to the psyche.
    pub fn will(mut self, will: Will<T>) -> Self {
        self.wills.push(will);
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
        let mut wit_streams = Vec::new();
        for wit in self.wits.iter_mut() {
            let sensors_for_wit: Vec<_> = self
                .sensors
                .iter()
                .map(|s| SharedSensor::new(s.clone()))
                .collect();
            let stream = wit.observe(sensors_for_wit).await;
            wit_streams.push(stream);
        }
        let mut will_streams = Vec::new();
        for will in self.wills.iter_mut() {
            for m in self.motors.iter() {
                will.register_motor(m.as_ref());
            }
            let sensors_for_will: Vec<_> = self
                .sensors
                .iter()
                .map(|s| SharedSensor::new(s.clone()))
                .collect();
            let stream = will.observe(sensors_for_will).await;
            will_streams.push(stream);
        }
        let mut merged_wits = stream::select_all(wit_streams);
        let mut merged_wills = stream::select_all(will_streams);
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("psyche shutting down");
                    break;
                }
                Some(batch) = merged_wits.next() => {
                    for impression in batch {
                        debug!(?impression.how, "impression received");
                        let text = impression.how.clone();
                        for motor in self.motors.iter() {
                            use futures::stream;
                            let t = text.clone();
                            let body = stream::once(async move { t }).boxed();
                            let mut action = Action::new("log", Value::Null, body);
                            action.intention.assigned_motor = motor.name().to_string();
                            if let Err(e) = motor.perform(action).await {
                                debug!(?e, "motor action failed");
                            }
                        }
                    }
                }
                Some(actions) = merged_wills.next() => {
                    for action in actions {
                        let target = action.intention.assigned_motor.clone();
                        if let Some(motor) = self.motors.iter().find(|m| m.name() == target) {
                            if let Err(e) = motor.perform(action).await {
                                debug!(?e, "motor action failed");
                            }
                        } else {
                            debug!(?target, "no motor for action");
                        }
                    }
                }
            }
        }
    }

    /// Process a single urge by dispatching it to the appropriate motor.
    pub async fn process_urge(&self, urge: Urge) -> Result<ActionResult, MotorError> {
        for motor in &self.motors {
            if motor.name() == urge.name {
                let body = if let Some(b) = &urge.body {
                    let text = b.clone();
                    futures::stream::once(async move { text }).boxed()
                } else {
                    futures::stream::empty().boxed()
                };
                let intention = Intention::assign(urge.clone(), motor.name().to_string());
                let action = Action { intention, body };
                return motor.perform(action).await;
            }
        }
        Err(MotorError::Unrecognized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LLMClient, TokenStream};
    use futures::{StreamExt, stream};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestSensor;
    impl Sensor<String> for TestSensor {
        fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
            let s = Sensation {
                kind: "t".into(),
                when: chrono::Local::now(),
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
        fn description(&self) -> &'static str {
            "counts how many actions were performed"
        }
        fn name(&self) -> &'static str {
            "log"
        }
        async fn perform(&self, _action: Action) -> Result<ActionResult, MotorError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ActionResult {
                sensations: Vec::new(),
                completed: true,
                completion: None,
                interruption: None,
            })
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

    #[tokio::test]
    async fn wits_and_wills_run_together() {
        #[derive(Clone)]
        struct TagLLM;

        #[async_trait::async_trait]
        impl LLMClient for TagLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let tokens = vec!["<log>".to_string(), "hi".to_string(), "</log>".to_string()];
                Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(TagLLM);
        let wit = Wit::new(llm.clone()).delay_ms(10);
        let will = Will::new(llm.clone()).delay_ms(10).motor("log", "count");
        let psyche = Psyche::new()
            .sensor(TestSensor)
            .wit(wit)
            .will(will)
            .motor(CountMotor(count.clone()));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn process_urge_routes_to_motor() {
        struct SenseMotor(&'static str);

        #[async_trait::async_trait]
        impl Motor for SenseMotor {
            fn description(&self) -> &'static str {
                "test motor"
            }
            fn name(&self) -> &'static str {
                self.0
            }
            async fn perform(&self, _action: Action) -> Result<ActionResult, MotorError> {
                Ok(ActionResult {
                    sensations: vec![Sensation {
                        kind: match self.0 {
                            "look" => "image/jpeg",
                            "listen" => "audio/mpeg",
                            "sniff" => "chemical/smell",
                            _ => "unknown",
                        }
                        .into(),
                        when: chrono::Local::now(),
                        what: serde_json::Value::Null,
                        source: None,
                    }],
                    completed: true,
                    completion: None,
                    interruption: None,
                })
            }
        }

        let psyche: Psyche = Psyche::new()
            .motor(SenseMotor("look"))
            .motor(SenseMotor("listen"))
            .motor(SenseMotor("sniff"));

        for (urge, kind) in [
            (
                Urge {
                    name: "look".into(),
                    args: HashMap::new(),
                    body: None,
                },
                "image/jpeg",
            ),
            (
                Urge {
                    name: "listen".into(),
                    args: HashMap::new(),
                    body: None,
                },
                "audio/mpeg",
            ),
            (
                Urge {
                    name: "sniff".into(),
                    args: HashMap::new(),
                    body: None,
                },
                "chemical/smell",
            ),
        ] {
            let res = psyche.process_urge(urge).await.unwrap();
            assert!(res.completed);
            assert_eq!(res.sensations[0].kind, kind);
        }
    }
}
