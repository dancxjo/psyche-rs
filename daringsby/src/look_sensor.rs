use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

use psyche_rs::{Sensation, Sensor};

/// Sensor that streams vision descriptions from the `look` motor.
///
/// The `look` motor sends `vision.description` sensations through a channel.
/// `LookSensor` exposes that channel as a [`Sensor`] so the Combobulator can
/// incorporate the descriptions into its situation timeline.
///
/// # Example
/// ```
/// use daringsby::look_sensor::LookSensor;
/// use psyche_rs::{Sensation, Sensor};
/// use tokio::sync::mpsc::unbounded_channel;
/// use futures::StreamExt;
///
/// #[tokio::main]
/// async fn main() {
///     let (tx, rx) = unbounded_channel();
///     let mut sensor = LookSensor::new(rx);
///     tx.send(vec![Sensation { kind: "vision.description".into(), when: chrono::Local::now(), what: "a cat".into(), source: None }]).unwrap();
///     let mut stream = sensor.stream();
///     let batch = stream.next().await.unwrap();
///     assert_eq!(batch[0].what, "a cat");
/// }
/// ```
pub struct LookSensor {
    rx: Option<UnboundedReceiver<Vec<Sensation<String>>>>,
}

impl LookSensor {
    /// Create a new sensor wrapping the provided receiver.
    pub fn new(rx: UnboundedReceiver<Vec<Sensation<String>>>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl Sensor<String> for LookSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        match self.rx.take() {
            Some(rx) => UnboundedReceiverStream::new(rx).boxed(),
            None => futures::stream::empty().boxed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn forwards_batches() {
        let (tx, rx) = unbounded_channel();
        let mut sensor = LookSensor::new(rx);
        let batch = vec![Sensation {
            kind: "vision.description".into(),
            when: chrono::Local::now(),
            what: "hi".into(),
            source: None,
        }];
        tx.send(batch.clone()).unwrap();
        let mut stream = sensor.stream();
        let got = stream.next().await.unwrap();
        assert_eq!(got[0].what, "hi");
    }
}
