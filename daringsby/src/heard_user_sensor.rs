use chrono::Local;
use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;

use psyche_rs::{Sensation, Sensor, render_template};

/// Sensor emitting sensations when the agent hears an interlocutor speaking.
/// Each utterance is passed through as-is by default so the voice receives
/// clean chat messages. A custom template may be provided if narrative phrasing
/// is desired.
pub struct HeardUserSensor {
    rx: Option<Receiver<String>>,
    template: String,
}

impl HeardUserSensor {
    /// Create a new sensor from the given broadcast receiver.
    pub fn new(rx: Receiver<String>) -> Self {
        Self::with_template(rx, "{text}")
    }

    /// Create a sensor with a custom phrase template.
    pub fn with_template(rx: Receiver<String>, template: impl Into<String>) -> Self {
        Self {
            rx: Some(rx),
            template: template.into(),
        }
    }
}

impl Sensor<String> for HeardUserSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        let rx = self
            .rx
            .take()
            .expect("HeardUserSensor stream called more than once");
        let template = self.template.clone();
        BroadcastStream::new(rx)
            .inspect_err(|e| tracing::warn!(error=?e, "HeardUserSensor receiver error"))
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
                    kind: "interlocutor_audio".into(),
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
        let mut sensor = HeardUserSensor::new(rx);
        tx.send("Hello".into()).unwrap();
        drop(tx);
        let start = Local::now();
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch[0].what, "Hello");
        assert_eq!(batch[0].kind, "interlocutor_audio");
        assert!(batch[0].when >= start && batch[0].when <= Local::now());
    }

    #[tokio::test]
    async fn custom_template_is_used() {
        let (tx, rx) = broadcast::channel(4);
        let mut sensor = HeardUserSensor::with_template(rx, "interlocutor said {text}");
        tx.send("Yo".into()).unwrap();
        drop(tx);
        let mut stream = sensor.stream();
        let batch = stream.next().await.unwrap();
        assert_eq!(batch[0].what, "interlocutor said Yo");
    }
}
