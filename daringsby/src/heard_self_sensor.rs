use chrono::Local;
use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;

use psyche_rs::{Sensation, Sensor, render_template};

/// Sensor emitting sensations when the agent hears its own speech.
/// Each text received is wrapped in a sentence describing the event.
pub struct HeardSelfSensor {
    rx: Option<Receiver<String>>,
    template: String,
}

impl HeardSelfSensor {
    /// Create a new sensor from the given broadcast receiver.
    pub fn new(rx: Receiver<String>) -> Self {
        Self::with_template(rx, "I heard myself say out loud: \"{text}\"")
    }

    /// Create a sensor with a custom phrase template. `{text}` will be replaced
    /// with the received utterance.
    pub fn with_template(rx: Receiver<String>, template: impl Into<String>) -> Self {
        Self {
            rx: Some(rx),
            template: template.into(),
        }
    }
}

impl Sensor<String> for HeardSelfSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        let rx = self
            .rx
            .take()
            .expect("HeardSelfSensor stream called more than once");
        let template = self.template.clone();
        BroadcastStream::new(rx)
            .inspect_err(|e| tracing::warn!(error = ?e, "HeardSelfSensor receiver error"))
            .filter_map(|msg| async move { msg.ok() })
            .map(move |text| {
                #[derive(serde::Serialize)]
                struct Ctx<'a> {
                    text: &'a str,
                }
                let what = render_template(&template, &Ctx { text: &text }).unwrap_or_else(|e| {
                    tracing::warn!(error=?e, "template render failed");
                    template.clone()
                });
                vec![Sensation {
                    when: Local::now(),
                    what,
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
        let start = Local::now();
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch[0].what, "I heard myself say out loud: \"Hello\"");
        assert!(batch[0].when >= start && batch[0].when <= Local::now());
    }

    #[tokio::test]
    async fn custom_template_is_used() {
        let (tx, rx) = broadcast::channel(4);
        let mut sensor = HeardSelfSensor::with_template(rx, "heard: {text}");
        tx.send("Yo".into()).unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch[0].what, "heard: Yo");
    }
}
