use std::sync::{Arc, Mutex};

use chrono::Utc;
use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, info, trace, warn};

#[cfg(test)]
use crate::{ActionResult, MotorError};
use crate::{
    Impression, MemoryStore, Motor, Sensation, Sensor, StoredImpression, StoredSensation, Will, Wit,
};

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
/// use psyche_rs::{Psyche, Wit, Sensor, LLMClient, LLMTokenStream, Sensation};
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
///     ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
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
    motors: Vec<Arc<dyn Motor + Send + Sync>>,
    wits: Vec<Wit<T>>,
    will: Option<Will<T>>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
}

impl<T> Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    /// Create an empty [`Psyche`].
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
            motors: Vec::new(),
            wits: Vec::new(),
            will: None,
            store: None,
        }
    }

    /// Add a sensor to the psyche.
    pub fn sensor(mut self, sensor: impl Sensor<T> + Send + 'static) -> Self {
        self.sensors.push(Arc::new(Mutex::new(sensor)));
        self
    }

    /// Add a motor to the psyche.
    pub fn motor(mut self, motor: impl Motor + 'static) -> Self {
        self.motors.push(Arc::new(motor));
        self
    }

    /// Add a wit to the psyche.
    pub fn wit(mut self, wit: Wit<T>) -> Self {
        self.wits.push(wit);
        self
    }

    /// Add a will to the psyche.
    pub fn will(mut self, will: Will<T>) -> Self {
        self.will = Some(will);
        self
    }

    /// Attach a memory store used to persist impressions.
    pub fn memory(mut self, store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        self.store = Some(store);
        self
    }
}

impl<T> Default for Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Psyche<T>
where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
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
        let mut merged_wits = stream::select_all(wit_streams);

        let will = self
            .will
            .as_mut()
            .expect("Psyche requires exactly one Will");
        for m in self.motors.iter() {
            will.register_motor(m.as_ref());
        }
        let sensors_for_will: Vec<_> = self
            .sensors
            .iter()
            .map(|s| SharedSensor::new(s.clone()))
            .collect();
        let stream = will.observe(sensors_for_will).await;
        let mut merged_wills = stream::select_all(vec![stream]);
        loop {
            trace!("psyche loop tick");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("psyche shutting down");
                    break;
                }
                Some(batch) = merged_wits.next() => {
                    trace!(batch_len = batch.len(), "psyche received impressions");
                    if let Some(store) = &self.store {
                        for imp in batch {
                            if let Err(e) = persist_impression(store.as_ref(), &imp, "Instant") {
                                warn!(?e, "failed to persist impression");
                            }
                        }
                    }
                }
                Some(intentions) = merged_wills.next() => {
                    trace!(count = intentions.len(), "psyche received intentions");
                    for intention in intentions {
                        debug!(?intention, "Psyche received intention");
                        let target = intention.assigned_motor.clone();
                        if let Some(motor) = self.motors.iter().find(|m| m.name() == target) {
                            debug!(target_motor = %motor.name(), "Psyche matched intention to motor");
                            let motor = motor.clone();
                            tokio::spawn(async move {
                                if let Err(e) = motor.perform(intention).await {
                                    debug!(?e, "motor action failed");
                                }
                            });
                        } else {
                            warn!(?intention, "Psyche could not match motor for intention");
                        }
                    }
                }
            }
        }
    }
}

fn persist_impression<T: serde::Serialize>(
    store: &dyn MemoryStore,
    imp: &Impression<T>,
    kind: &str,
) -> anyhow::Result<()> {
    let mut sensation_ids = Vec::new();
    for s in &imp.what {
        let sid = uuid::Uuid::new_v4().to_string();
        sensation_ids.push(sid.clone());
        let stored = StoredSensation {
            id: sid,
            kind: s.kind.clone(),
            when: s.when.with_timezone(&Utc),
            data: serde_json::to_string(&s.what)?,
        };
        store.store_sensation(&stored)?;
    }
    let stored_imp = StoredImpression {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        when: Utc::now(),
        how: imp.how.clone(),
        sensation_ids,
        impression_ids: Vec::new(),
    };
    store.store_impression(&stored_imp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Intention, LLMClient, LLMTokenStream};
    use futures::{StreamExt, stream};
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

    struct CountMotor(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl Motor for CountMotor {
        fn description(&self) -> &'static str {
            "counts how many actions were performed"
        }
        fn name(&self) -> &'static str {
            "log"
        }
        async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
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
        #[derive(Clone)]
        struct TagLLM;

        #[async_trait::async_trait]
        impl LLMClient for TagLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let tokens = vec!["<log>".to_string(), "hi".to_string(), "</log>".to_string()];
                Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(TagLLM);
        let will = Will::new(llm.clone()).delay_ms(10).motor("log", "count");
        let psyche = Psyche::new()
            .sensor(TestSensor)
            .will(will)
            .motor(CountMotor(count.clone()));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
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
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
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
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn will_actions_dispatched_to_motors() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct MultiLLM;

        #[async_trait::async_trait]
        impl LLMClient for MultiLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let tokens = vec![
                    "<say>".to_string(),
                    "Hello".to_string(),
                    "</say><log>".to_string(),
                    "Logging this".to_string(),
                    "</log>".to_string(),
                ];
                Ok(Box::pin(stream::iter(tokens.into_iter().map(Ok))))
            }
        }

        struct DummySensor;
        impl Sensor<String> for DummySensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                use futures::stream;
                let s = Sensation {
                    kind: "t".into(),
                    when: chrono::Local::now(),
                    what: "foo".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        struct CountingMotor(Arc<AtomicUsize>, &'static str);
        #[async_trait::async_trait]
        impl Motor for CountingMotor {
            fn description(&self) -> &'static str {
                "count"
            }
            fn name(&self) -> &'static str {
                self.1
            }
            async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(ActionResult::default())
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(MultiLLM);
        let will = Will::new(llm.clone())
            .delay_ms(10)
            .motor("say", "speak")
            .motor("log", "record");
        let psyche = Psyche::new()
            .sensor(DummySensor)
            .will(will)
            .motor(CountingMotor(count.clone(), "say"))
            .motor(CountingMotor(count.clone(), "log"));

        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn actions_do_not_block_loop() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct MultiActionLLM;

        #[async_trait::async_trait]
        impl LLMClient for MultiActionLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let toks = vec![
                    "<log>".to_string(),
                    "1".to_string(),
                    "</log>".to_string(),
                    "<log>".to_string(),
                    "2".to_string(),
                    "</log>".to_string(),
                ];
                Ok(Box::pin(stream::iter(toks.into_iter().map(Ok))))
            }
        }

        struct DummySensor;
        impl Sensor<String> for DummySensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                use futures::stream;
                let s = Sensation {
                    kind: "t".into(),
                    when: chrono::Local::now(),
                    what: "foo".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        struct SlowMotor(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl Motor for SlowMotor {
            fn description(&self) -> &'static str {
                "slow"
            }
            fn name(&self) -> &'static str {
                "log"
            }
            async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(ActionResult::default())
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let llm = Arc::new(MultiActionLLM);
        let will = Will::new(llm.clone()).delay_ms(10).motor("log", "count");
        let psyche = Psyche::new()
            .sensor(DummySensor)
            .will(will)
            .motor(SlowMotor(count.clone()));

        let _ = tokio::time::timeout(std::time::Duration::from_millis(250), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    #[should_panic(expected = "Will")]
    async fn run_requires_will() {
        let psyche: Psyche = Psyche::new();
        let _ = psyche.run().await;
    }

    #[tokio::test]
    async fn later_will_overrides_previous() {
        #[derive(Clone)]
        struct FirstLLM;

        #[async_trait::async_trait]
        impl LLMClient for FirstLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let toks = vec!["<a>".to_string(), "hi".to_string(), "</a>".to_string()];
                Ok(Box::pin(stream::iter(toks.into_iter().map(Ok))))
            }
        }

        #[derive(Clone)]
        struct SecondLLM;

        #[async_trait::async_trait]
        impl LLMClient for SecondLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                let toks = vec!["<b>".to_string(), "bye".to_string(), "</b>".to_string()];
                Ok(Box::pin(stream::iter(toks.into_iter().map(Ok))))
            }
        }

        struct CountMotor(Arc<AtomicUsize>, &'static str);
        #[async_trait::async_trait]
        impl Motor for CountMotor {
            fn description(&self) -> &'static str {
                "count"
            }
            fn name(&self) -> &'static str {
                self.1
            }
            async fn perform(&self, _intention: Intention) -> Result<ActionResult, MotorError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(ActionResult::default())
            }
        }

        let first = Will::<String>::new(Arc::new(FirstLLM))
            .delay_ms(10)
            .motor("a", "count");
        let second = Will::<String>::new(Arc::new(SecondLLM))
            .delay_ms(10)
            .motor("b", "count");
        struct DummySensor;
        impl Sensor<String> for DummySensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                use futures::stream;
                let s = Sensation {
                    kind: "t".into(),
                    when: chrono::Local::now(),
                    what: "foo".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }
        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));
        let psyche = Psyche::new()
            .sensor(DummySensor)
            .will(first)
            .will(second)
            .motor(CountMotor(count_a.clone(), "a"))
            .motor(CountMotor(count_b.clone(), "b"));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
        assert_eq!(count_a.load(Ordering::SeqCst), 0);
        assert!(count_b.load(Ordering::SeqCst) > 0);
    }
}
