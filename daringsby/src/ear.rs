use std::sync::Arc;

use futures::{
    StreamExt,
    stream::{BoxStream, select},
};

use crate::{HeardSelfSensor, HeardUserSensor, SpeechStream};
use psyche_rs::{Sensation, Sensor};

/// Sensor merging self and interlocutor speech.
///
/// The returned stream yields batches from either the self-heard or
/// interlocutor-heard sensors as soon as they arrive.
///
/// # Examples
/// ```ignore
/// use daringsby::{Ear, SpeechStream, HeardSelfSensor, HeardUserSensor};
/// use std::sync::Arc;
/// # use tokio::sync::broadcast;
/// # let (tx, rx) = broadcast::channel(1);
/// # let (utx, urx) = broadcast::channel(1);
/// # let stream = Arc::new(SpeechStream::new(rx, utx.subscribe(), urx));
/// let mut ear = Ear::new(
///     HeardSelfSensor::new(stream.subscribe_heard()),
///     HeardUserSensor::new(stream.subscribe_user()),
/// );
/// let _stream = ear.stream();
/// ```
pub struct Ear {
    self_sensor: HeardSelfSensor,
    interlocutor_sensor: HeardUserSensor,
}

impl Ear {
    /// Construct an [`Ear`] from the provided sensors.
    pub fn new(self_sensor: HeardSelfSensor, interlocutor_sensor: HeardUserSensor) -> Self {
        Self {
            self_sensor,
            interlocutor_sensor,
        }
    }

    /// Build an [`Ear`] subscribing to the given [`SpeechStream`].
    pub fn from_stream(stream: Arc<SpeechStream>) -> Self {
        Self {
            self_sensor: HeardSelfSensor::new(stream.subscribe_heard()),
            interlocutor_sensor: HeardUserSensor::new(stream.subscribe_user()),
        }
    }
}

impl Sensor<String> for Ear {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        let a = self.self_sensor.stream();
        let b = self.interlocutor_sensor.stream();
        select(a, b).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn forwards_both_sensors() {
        let (self_tx, self_rx) = broadcast::channel(4);
        let (user_tx, user_rx) = broadcast::channel(4);
        let mut ear = Ear::new(HeardSelfSensor::new(self_rx), HeardUserSensor::new(user_rx));
        self_tx.send("me".into()).unwrap();
        user_tx.send("you".into()).unwrap();
        drop(self_tx);
        drop(user_tx);
        let mut stream = ear.stream();
        let mut heard = Vec::new();
        while let Some(batch) = stream.next().await {
            heard.extend(batch.into_iter().map(|s| s.what));
        }
        assert!(heard.iter().any(|t| t.contains("me")));
        assert!(heard.iter().any(|t| t.contains("you")));
    }
}
