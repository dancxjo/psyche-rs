use futures::{StreamExt, stream::BoxStream};
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;

use psyche_rs::{Sensation, Sensor};

/// Sensor emitting sensations when the agent hears its own speech.
/// Each text received is wrapped in a sentence describing the event.
pub struct HeardSelfSensor {
    rx: Option<Receiver<String>>,
}

impl HeardSelfSensor {
    /// Create a new sensor from the given broadcast receiver.
    pub fn new(rx: Receiver<String>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl Sensor<String> for HeardSelfSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        let rx = self.rx.take().expect("stream may only be called once");
        BroadcastStream::new(rx)
            .filter_map(|msg| async move { msg.ok() })
            .map(|text| {
                vec![Sensation {
                    kind: "self_audio".into(),
                    when: chrono::Local::now(),
                    what: format!("I just heard myself say out loud: '{}'", text),
                    source: None,
                }]
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn emits_sensation_per_message() {
        let (tx, rx) = broadcast::channel(4);
        let mut sensor = HeardSelfSensor::new(rx);
        tx.send("Hello".into()).unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch[0].what, "I just heard myself say out loud: 'Hello'");
    }
}
