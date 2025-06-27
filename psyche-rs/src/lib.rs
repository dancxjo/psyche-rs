//! Core types for the `psyche-rs` crate.
//!
//! This crate currently exposes [`Sensation`], [`Impression`], [`Sensor`] and
//! [`Witness`] building blocks for constructing artificial agents.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

/// A generic Sensation type for the Pete runtime.
///
/// The payload type `T` defaults to [`serde_json::Value`].
///
/// # Examples
///
/// Creating a typed sensation:
///
/// ```
/// use chrono::Utc;
/// use psyche_rs::Sensation;
///
/// let s: Sensation<String> = Sensation {
///     kind: "utterance.text".into(),
///     when: Utc::now(),
///     what: "hello".into(),
///     source: Some("interlocutor".into()),
/// };
/// assert_eq!(s.what, "hello");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensation<T = serde_json::Value> {
    /// Category of sensation, e.g. `"utterance.text"`.
    pub kind: String,
    /// Timestamp for when the sensation occurred.
    pub when: DateTime<Utc>,
    /// Payload describing what was sensed.
    pub what: T,
    /// Optional origin identifier.
    pub source: Option<String>,
}

/// A high-level summary of one or more sensations.
///
/// `Impression` bundles related sensations in [`what`] and expresses
/// them in natural language via [`how`].
///
/// # Examples
///
/// ```
/// use chrono::Utc;
/// use psyche_rs::{Impression, Sensation};
///
/// let what = vec![Sensation::<String> {
///     kind: "utterance.text".into(),
///     when: Utc::now(),
///     what: "salutations".into(),
///     source: None,
/// }];
/// let impression = Impression {
///     what: what.clone(),
///     how: "He said salutations".into(),
/// };
/// assert_eq!(impression.what[0].kind, "utterance.text");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impression<T = serde_json::Value> {
    /// Sensations that led to this impression.
    pub what: Vec<Sensation<T>>,
    /// Natural language summarizing the sensations.
    pub how: String,
}

/// A source of sensations.
pub trait Sensor<T = serde_json::Value> {
    /// Returns a stream of sensation batches.
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<T>>>;
}

/// Consumes sensors and emits impressions.
///
/// `Witness` takes ownership of provided sensors and outputs batches of
/// [`Impression`]s.  Callers remain free to box or share sensor instances as
/// desired and simply pass them as a `Vec`.
#[async_trait(?Send)]
pub trait Witness<T = serde_json::Value> {
    /// Observes the provided sensors and yields impression batches.
    async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + 'static;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};

    #[tokio::test]
    async fn impression_from_sensor() {
        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "test".into(),
                    when: Utc::now(),
                    what: "ping".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        struct TestWitness;

        #[async_trait(?Send)]
        impl Witness<String> for TestWitness {
            async fn observe<S>(
                &mut self,
                mut sensors: Vec<S>,
            ) -> BoxStream<'static, Vec<Impression<String>>>
            where
                S: Sensor<String> + 'static,
            {
                let impressions = sensors.pop().unwrap().stream().map(|what| {
                    vec![Impression {
                        how: format!("{} event", what[0].what),
                        what,
                    }]
                });
                impressions.boxed()
            }
        }

        let mut witness = TestWitness;
        let s = TestSensor;
        let mut stream = witness.observe(vec![s]).await;
        if let Some(impressions) = stream.next().await {
            assert_eq!(impressions[0].how, "ping event");
        } else {
            panic!("no impression emitted");
        }
    }
}
