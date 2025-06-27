use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tracing::{debug, info};

use crate::{Impression, Motor, MotorCommand, Sensation, Sensor, Witness};

#[async_trait(?Send)]
trait WitnessRunner<T> {
    async fn observe_boxed(
        &mut self,
        sensors: Vec<SharedSensor<T>>,
    ) -> BoxStream<'static, Vec<Impression<T>>>;
}

struct BoxedWitness<T, W: Witness<T> + Send> {
    inner: W,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, W> BoxedWitness<T, W>
where
    W: Witness<T> + Send,
{
    fn new(inner: W) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait(?Send)]
impl<T, W> WitnessRunner<T> for BoxedWitness<T, W>
where
    T: Clone + Default + Send + 'static + serde::Serialize,
    W: Witness<T> + Send,
{
    async fn observe_boxed(
        &mut self,
        sensors: Vec<SharedSensor<T>>,
    ) -> BoxStream<'static, Vec<Impression<T>>> {
        self.inner.observe(sensors).await
    }
}

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
    motors: Vec<Box<dyn Motor<T> + Send>>,
    wits: Vec<Box<dyn WitnessRunner<T> + Send>>,
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
    pub fn motor(mut self, motor: impl Motor<T> + Send + 'static) -> Self {
        self.motors.push(Box::new(motor));
        self
    }

    /// Add a wit to the psyche.
    pub fn wit<W>(mut self, wit: W) -> Self
    where
        W: Witness<T> + Send + 'static,
    {
        self.wits.push(Box::new(BoxedWitness::new(wit)));
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
            let stream = wit.observe_boxed(sensors_for_wit).await;
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
                        for motor in self.motors.iter_mut() {
                            let cmd = MotorCommand::<T> { name: "log".into(), args: T::default(), content: Some(impression.how.clone()) };
                            motor.execute(cmd).await;
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
    use async_trait::async_trait;
    use futures::StreamExt;
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

    struct TestWit;
    #[async_trait(?Send)]
    impl Witness<String> for TestWit {
        async fn observe<S>(
            &mut self,
            mut sensors: Vec<S>,
        ) -> BoxStream<'static, Vec<Impression<String>>>
        where
            S: Sensor<String> + Send + 'static,
        {
            sensors
                .pop()
                .unwrap()
                .stream()
                .map(|s| {
                    vec![Impression {
                        how: s[0].what.clone(),
                        what: s,
                    }]
                })
                .boxed()
        }
    }

    struct CountMotor(Arc<AtomicUsize>);
    #[async_trait(?Send)]
    impl Motor<String> for CountMotor {
        async fn execute(&mut self, _cmd: MotorCommand<String>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn psyche_runs() {
        let count = Arc::new(AtomicUsize::new(0));
        let psyche = Psyche::new()
            .sensor(TestSensor)
            .wit(TestWit)
            .motor(CountMotor(count.clone()));
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), psyche.run()).await;
        assert!(count.load(Ordering::SeqCst) > 0);
    }
}
