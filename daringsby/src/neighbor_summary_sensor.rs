use futures::{StreamExt, stream::BoxStream};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

use psyche_rs::{Sensation, Sensor};

/// Sensor that streams neighbor summary sensations from a channel.
pub struct NeighborSummarySensor {
    rx: Option<UnboundedReceiver<Vec<Sensation<String>>>>,
}

impl NeighborSummarySensor {
    /// Create a new sensor wrapping the provided receiver.
    pub fn new(rx: UnboundedReceiver<Vec<Sensation<String>>>) -> Self {
        Self { rx: Some(rx) }
    }
}

impl Sensor<String> for NeighborSummarySensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
        match self.rx.take() {
            Some(rx) => UnboundedReceiverStream::new(rx).boxed(),
            None => futures::stream::empty().boxed(),
        }
    }
}
