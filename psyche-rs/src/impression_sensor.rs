use chrono::Utc;
use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{Impression, Sensation, Sensor};

/// Sensor that converts impression batches into sensations.
pub struct ImpressionSensor<T> {
    rx: Option<UnboundedReceiver<Vec<Impression<T>>>>,
}

impl<T> ImpressionSensor<T> {
    /// Creates a new sensor wrapping the provided receiver.
    pub fn new(rx: UnboundedReceiver<Vec<Impression<T>>>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl<T> Sensor<Impression<T>> for ImpressionSensor<T>
where
    T: Clone + Send + 'static,
{
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<Impression<T>>>> {
        let rx = self.rx.take().expect("stream may only be called once");
        UnboundedReceiverStream::new(rx)
            .map(|batch| {
                batch
                    .into_iter()
                    .map(|imp| Sensation {
                        kind: "impression".into(),
                        when: Utc::now(),
                        what: imp,
                        source: None,
                    })
                    .collect()
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn forwards_impressions_as_sensations() {
        let (tx, rx) = unbounded_channel();
        let mut sensor = ImpressionSensor::new(rx);
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
}
