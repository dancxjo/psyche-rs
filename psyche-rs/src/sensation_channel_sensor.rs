use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{Sensation, Sensor};

/// Sensor backed by an unbounded channel of sensation batches.
///
/// This is useful for feeding programmatically generated sensations
/// back into a [`Witness`].
///
/// # Examples
/// ```no_run
/// use psyche_rs::{Sensation, SensationSensor};
/// use tokio::sync::mpsc::unbounded_channel;
///
/// let (tx, rx) = unbounded_channel();
/// let mut sensor = SensationSensor::new(rx);
/// tx.send(vec![Sensation::<String> {
///     kind: "note".into(),
///     when: chrono::Utc::now(),
///     what: "ping".into(),
///     source: None,
/// }]).unwrap();
/// drop(tx);
/// // async context: sensor.stream().next().await
/// ```
pub struct SensationSensor<T> {
    rx: Option<UnboundedReceiver<Vec<Sensation<T>>>>,
}

impl<T> SensationSensor<T> {
    /// Creates a new sensor wrapping the provided receiver.
    pub fn new(rx: UnboundedReceiver<Vec<Sensation<T>>>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl<T> Sensor<T> for SensationSensor<T>
where
    T: Clone + Send + 'static,
{
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<T>>> {
        let rx = self.rx.take().expect("stream may only be called once");
        UnboundedReceiverStream::new(rx).boxed()
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
        let mut sensor = SensationSensor::new(rx);
        let sensation = Sensation {
            kind: "test".into(),
            when: chrono::Utc::now(),
            what: "hi".to_string(),
            source: None,
        };
        tx.send(vec![sensation.clone()]).unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].kind, "test");
        assert_eq!(batch[0].what, "hi");
    }
}
