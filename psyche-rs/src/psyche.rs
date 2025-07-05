use std::sync::{Arc, Mutex};

use chrono::Utc;
use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, error, info, trace, warn};

use crate::Intention;

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
///     async fn embed(
///         &self,
///         _text: &str,
///     ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
///         Ok(vec![0.0])
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
    ears: Vec<Arc<Mutex<dyn Sensor<String> + Send>>>,
    motors: Vec<Arc<dyn Motor + Send + Sync>>,
    wits: Vec<Wit<T>>,
    will: Option<Will<T>>,
    voice: Option<crate::Voice>,
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
            ears: Vec::new(),
            motors: Vec::new(),
            wits: Vec::new(),
            will: None,
            voice: None,
            store: None,
        }
    }

    /// Add a sensor to the psyche.
    pub fn sensor(mut self, sensor: impl Sensor<T> + Send + 'static) -> Self {
        self.sensors.push(Arc::new(Mutex::new(sensor)));
        self
    }

    /// Add an ear sensor used by [`Voice`].
    pub fn ear(mut self, sensor: impl Sensor<String> + Send + 'static) -> Self {
        self.ears.push(Arc::new(Mutex::new(sensor)));
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

    /// Add a voice to the psyche.
    pub fn voice(mut self, voice: crate::Voice) -> Self {
        self.voice = Some(voice);
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
        let merged_wits = stream::select_all(wit_streams);

        let will = self
            .will
            .as_mut()
            .expect("Psyche requires exactly one Will");
        for m in self.motors.iter() {
            will.register_motor(m.as_ref());
        }
        let executor = crate::MotorExecutor::new(self.motors.clone(), 4, 16, self.store.clone());
        let sensors_for_will: Vec<_> = self
            .sensors
            .iter()
            .map(|s| SharedSensor::new(s.clone()))
            .collect();
        let stream = will.observe(sensors_for_will).await;
        let mut intention_streams: Vec<BoxStream<'static, Vec<Intention>>> = vec![stream];

        if let Some(voice) = &self.voice {
            if let Some(sensor) = self.ears.first() {
                let ear = SharedSensor::new(sensor.clone());
                let window = will.window_arc();
                let get_situation = Arc::new(move || crate::build_timeline(&window));
                let latest = will.latest_instant_arc();
                let get_instant = Arc::new(move || latest.lock().unwrap().clone());
                let vstream = voice.observe(ear, get_situation, get_instant).await;
                intention_streams.push(vstream);
            }
        }
        let merged_wills = stream::select_all(intention_streams);

        use crate::psyche_event::PsycheEvent;
        let mut merged = stream::select(
            merged_wits.map(PsycheEvent::Impressions),
            merged_wills.map(PsycheEvent::Intentions),
        );

        loop {
            trace!("psyche loop tick");
            tokio::select! {
                _ = crate::shutdown_signal() => {
                    info!("psyche shutting down");
                    break;
                }
                Some(event) = merged.next() => {
                    match event {
                        PsycheEvent::Impressions(batch) => {
                            trace!(batch_len = batch.len(), "psyche received impressions");
                            if let Some(store) = &self.store {
                                for imp in batch {
                                    if let Err(e) = persist_impression(store.as_ref(), &imp, "Instant").await {
                                        warn!(?e, "failed to persist impression");
                                    }
                                }
                            }
                        }
                        PsycheEvent::Intentions(intentions) => {
                            trace!(count = intentions.len(), "psyche received intentions");
                            for intention in intentions {
                                debug!(?intention, "Psyche received intention");
                                executor.spawn_intention(intention);
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn persist_impression<T: serde::Serialize>(
    store: &(dyn MemoryStore + Send + Sync),
    imp: &Impression<T>,
    kind: &str,
) -> anyhow::Result<()> {
    debug!("persisting impression");
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
        store.store_sensation(&stored).await.map_err(|e| {
            error!(?e, "store_sensation failed");
            e
        })?;
    }
    let stored_imp = StoredImpression {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        when: Utc::now(),
        how: imp.how.clone(),
        sensation_ids,
        impression_ids: Vec::new(),
    };
    store.store_impression(&stored_imp).await.map_err(|e| {
        error!(?e, "store_impression failed");
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Intention;
    use futures::{StreamExt, stream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    /*
    #[tokio::test]
    async fn psyche_runs() {
        #[derive(Clone)]
        struct TagLLM;

        #[async_trait::async_trait]
        impl LLMClient for TagLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                use crate::llm::types::Token;
                let tokens = vec![
                    Token { text: "<log>".into() },
                    Token { text: "hi".into() },
                    Token { text: "</log>".into() },
                ];
                Ok(Box::pin(stream::iter(tokens)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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
    */

    /*
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
                use crate::llm::types::Token;
                let tokens = vec![
                    Token { text: "<log>".into() },
                    Token { text: "hi".into() },
                    Token { text: "</log>".into() },
                ];
                Ok(Box::pin(stream::iter(tokens)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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
    */

    /*
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
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                use crate::llm::types::Token;
                let tokens = vec![
                    Token { text: "<speak>".into() },
                    Token { text: "Hello".into() },
                    Token { text: "</speak><log>".into() },
                    Token { text: "Logging this".into() },
                    Token { text: "</log>".into() },
                ];
                Ok(Box::pin(stream::iter(tokens)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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
            .motor("speak", "speak")
            .motor("log", "record");
        let psyche = Psyche::new()
            .sensor(DummySensor)
            .will(will)
            .motor(CountingMotor(count.clone(), "speak"))
            .motor(CountingMotor(count.clone(), "log"));

        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) >= 2);
    }
    */

    /*
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
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                use crate::llm::types::Token;
                let toks = vec![
                    Token { text: "<log>".into() },
                    Token { text: "1".into() },
                    Token { text: "</log>".into() },
                    Token { text: "<log>".into() },
                    Token { text: "2".into() },
                    Token { text: "</log>".into() },
                ];
                Ok(Box::pin(stream::iter(toks)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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
    */

    #[tokio::test]
    #[should_panic(expected = "Will")]
    async fn run_requires_will() {
        let psyche: Psyche = Psyche::new();
        let _ = psyche.run().await;
    }

    /*
    #[tokio::test]
    async fn later_will_overrides_previous() {
        #[derive(Clone)]
        struct FirstLLM;

        #[async_trait::async_trait]
        impl LLMClient for FirstLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                use crate::llm::types::Token;
                let toks = vec![
                    Token { text: "<a>".into() },
                    Token { text: "hi".into() },
                    Token { text: "</a>".into() },
                ];
                Ok(Box::pin(stream::iter(toks)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
            }
        }

        #[derive(Clone)]
        struct SecondLLM;

        #[async_trait::async_trait]
        impl LLMClient for SecondLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use futures::stream;
                use crate::llm::types::Token;
                let toks = vec![
                    Token { text: "<b>".into() },
                    Token { text: "bye".into() },
                    Token { text: "</b>".into() },
                ];
                Ok(Box::pin(stream::iter(toks)))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
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
    */

    /*
    #[tokio::test]
    async fn llm_calls_run_in_parallel() {
        use tokio::sync::Barrier;

        #[derive(Clone)]
        struct BarrierLLM(Arc<Barrier>);

        #[async_trait::async_trait]
        impl LLMClient for BarrierLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                self.0.wait().await;
                use futures::stream;
                use crate::llm::types::Token;
                Ok(Box::pin(stream::once(async {
                    Token { text: "<log></log>".into() }
                })))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                self.0.wait().await;
                Ok(vec![0.0])
            }
        }

        let barrier = Arc::new(Barrier::new(3));
        let llm = Arc::new(BarrierLLM(barrier.clone()));
        let wit = Wit::new(llm.clone()).delay_ms(10);
        let will = Will::new(llm.clone()).delay_ms(10).motor("log", "count");
        let psyche = Psyche::new()
            .sensor(TestSensor)
            .wit(wit)
            .will(will)
            .motor(CountMotor(Arc::new(AtomicUsize::new(0))));

        let run = tokio::spawn(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(200), psyche.run()).await;
        });

        tokio::time::timeout(std::time::Duration::from_millis(100), barrier.wait())
            .await
            .expect("llm calls did not start in parallel");

        run.await.unwrap();
    }
    */
}
