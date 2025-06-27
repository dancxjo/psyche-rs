use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::{Impression, sensor::Sensor};

/// Consumes sensors and emits impressions.
///
/// `Witness` takes ownership of provided sensors and outputs batches of
/// [`Impression`]s. Callers remain free to box or share sensor instances as
/// desired and simply pass them as a `Vec`.
#[async_trait(?Send)]
pub trait Witness<T = serde_json::Value> {
    /// Observes the provided sensors and yields impression batches.
    async fn observe<S>(&mut self, sensors: Vec<S>) -> BoxStream<'static, Vec<Impression<T>>>
    where
        S: Sensor<T> + 'static;
}
