use futures::stream::BoxStream;

use crate::sensation::Sensation;

/// A source of sensations.
pub trait Sensor<T = serde_json::Value> {
    /// Returns a stream of sensation batches.
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<T>>>;
}

impl<T> Sensor<T> for Box<dyn Sensor<T> + Send> {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<T>>> {
        (**self).stream()
    }
}
