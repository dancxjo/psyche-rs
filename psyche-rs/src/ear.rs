use crate::memory::Sensation;
use tokio::sync::mpsc;

/// Pete's auditory feedback loop.
///
/// [`Ear`] feeds Pete's own speech back into the perception pipeline so it can
/// be remembered like any other [`Sensation`].
#[derive(Clone)]
pub struct Ear {
    sender: mpsc::Sender<Sensation>,
}

impl Ear {
    /// Create a new [`Ear`] sending sensations to `sender`.
    pub fn new(sender: mpsc::Sender<Sensation>) -> Self {
        Self { sender }
    }

    /// Record Pete's own utterance.
    pub async fn hear_self(&self, phrase: &str) {
        // Errors are ignored here as hearing one's own voice is not critical for
        // operation.
        let _ = self.sender.send(Sensation::new_text(phrase, "pete")).await;
    }
}
