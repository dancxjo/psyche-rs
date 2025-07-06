use tokio::sync::mpsc;
use tracing::warn;

/// Sender for bounded genius input channels.
///
/// Messages are dropped when the queue is full. A warning is logged with the
/// genius name.
#[derive(Clone)]
pub struct GeniusSender<I> {
    tx: mpsc::Sender<I>,
    name: &'static str,
}

impl<I> GeniusSender<I> {
    /// Try sending a message to the genius.
    ///
    /// Logs a warning if the queue is full.
    pub fn send(&self, msg: I) {
        if let Err(e) = self.tx.try_send(msg) {
            warn!(genius = self.name, ?e, "genius queue full; dropping input");
        }
    }
}

/// Create a bounded channel for genius inputs with the given capacity.
///
/// The returned [`GeniusSender`] should be used to enqueue inputs.
pub fn bounded_channel<I>(
    capacity: usize,
    name: &'static str,
) -> (GeniusSender<I>, mpsc::Receiver<I>) {
    let (tx, rx) = mpsc::channel(capacity);
    (GeniusSender { tx, name }, rx)
}
