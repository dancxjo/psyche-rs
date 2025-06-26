use crate::memory::Intention;
use tokio::sync::mpsc;

/// Events sent to a [`Motor`] instance.
#[derive(Debug, Clone)]
pub enum MotorEvent {
    /// Begin executing the given [`Intention`].
    Begin(Intention),
    /// Streaming chunk of output or feedback.
    Chunk(String),
    /// Signal that processing has finished.
    End,
}

impl PartialEq for MotorEvent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (MotorEvent::Begin(_), MotorEvent::Begin(_)) => true,
            (MotorEvent::Chunk(a), MotorEvent::Chunk(b)) => a == b,
            (MotorEvent::End, MotorEvent::End) => true,
            _ => false,
        }
    }
}

impl Eq for MotorEvent {}

#[async_trait::async_trait]
pub trait Motor: Send + Sync {
    async fn handle(&self, mut rx: mpsc::Receiver<MotorEvent>) -> anyhow::Result<()>;
}

/// A basic motor implementation used for testing. It simply logs all received
/// [`MotorEvent`]s.
pub struct DummyMotor {
    pub log: std::sync::Arc<tokio::sync::Mutex<Vec<MotorEvent>>>,
}

impl DummyMotor {
    pub fn new() -> Self {
        Self {
            log: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

impl Clone for DummyMotor {
    fn clone(&self) -> Self {
        Self {
            log: self.log.clone(),
        }
    }
}

#[async_trait::async_trait]
impl Motor for DummyMotor {
    async fn handle(&self, mut rx: mpsc::Receiver<MotorEvent>) -> anyhow::Result<()> {
        while let Some(evt) = rx.recv().await {
            self.log.lock().await.push(evt);
        }
        Ok(())
    }
}
