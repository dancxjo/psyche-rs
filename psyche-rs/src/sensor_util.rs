use chrono::Local;
use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{Impression, Sensation, Sensor};

/// Adapter bridging an impression channel into a [`Sensor`] stream.
///
/// This helper does not sense directly; it simply converts batches
/// received on a channel into [`Sensation`] events so they can be
/// consumed by other systems.
///
/// This sensor streams only once. Calling [`stream`][Self::stream] again
/// returns an empty stream rather than panicking.
///
/// ```no_run
/// use psyche_rs::{ImpressionStreamSensor, Impression, Sensation};
/// use tokio::sync::mpsc::unbounded_channel;
///
/// let (tx, rx) = unbounded_channel();
/// let mut sensor = ImpressionStreamSensor::new(rx);
/// tx.send(vec![Impression { how: "hi".into(), what: Vec::<Sensation<String>>::new() }]).unwrap();
/// drop(tx);
/// // async context: sensor.stream().next().await
/// ```
pub struct ImpressionStreamSensor<T> {
    rx: Option<UnboundedReceiver<Vec<Impression<T>>>>,
}

impl<T> ImpressionStreamSensor<T> {
    /// Creates a new sensor wrapping the provided receiver.
    #[must_use]
    pub fn new(rx: UnboundedReceiver<Vec<Impression<T>>>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl<T> Sensor<Impression<T>> for ImpressionStreamSensor<T>
where
    T: Clone + Send + 'static,
{
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<Impression<T>>>> {
        let rx = match self.rx.take() {
            Some(r) => r,
            None => return futures::stream::empty().boxed(),
        };
        UnboundedReceiverStream::new(rx)
            .map(|batch| batch.into_iter().map(build_sensation).collect())
            .boxed()
    }
}

fn build_sensation<T>(imp: Impression<T>) -> Sensation<Impression<T>> {
    Sensation {
        kind: "impression".into(),
        when: Local::now(),
        what: imp,
        source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn forwards_impressions_as_sensations() {
        let (tx, rx) = unbounded_channel::<Vec<Impression<()>>>();
        let mut sensor = ImpressionStreamSensor::new(rx);
        let imp = Impression {
            how: "hi".into(),
            what: Vec::<Sensation<()>>::new(),
        };
        tx.send(vec![imp]).unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].kind, "impression");
        assert_eq!(batch[0].what.how, "hi");
    }

    #[tokio::test]
    async fn streams_multiple_batches() {
        let (tx, rx) = unbounded_channel::<Vec<Impression<String>>>();
        let mut sensor = ImpressionStreamSensor::new(rx);
        tx.send(vec![Impression {
            how: "a".into(),
            what: Vec::new(),
        }])
        .unwrap();
        tx.send(vec![Impression {
            how: "b".into(),
            what: Vec::new(),
        }])
        .unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let a = stream.next().await.unwrap();
        let b = stream.next().await.unwrap();
        assert_eq!(a[0].what.how, "a");
        assert_eq!(b[0].what.how, "b");
        assert!(stream.next().await.is_none());
    }
}
