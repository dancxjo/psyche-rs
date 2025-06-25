use crate::memory::{Completion, Intention, Interruption};

#[async_trait::async_trait]
pub trait MotorSystem: Send + Sync {
    async fn invoke(&self, intention: &Intention) -> anyhow::Result<MotorFeedback>;
}

/// Feedback returned from a [`MotorSystem`] invocation.
pub enum MotorFeedback {
    /// The intention completed successfully with the given result.
    Completed(Completion),
    /// The intention was interrupted and did not complete.
    Interrupted(Interruption),
}

/// A basic motor implementation used for testing. It simply prints the intended
/// command in an XML-like format.
pub struct DummyMotor;

#[async_trait::async_trait]
impl MotorSystem for DummyMotor {
    async fn invoke(&self, intention: &Intention) -> anyhow::Result<MotorFeedback> {
        println!("<{} {:?}/>", intention.motor_name, intention.parameters);
        let comp = Completion {
            uuid: uuid::Uuid::new_v4(),
            intention: intention.uuid,
            outcome: "success".to_string(),
            transcript: Some(format!("Executed: {}", intention.motor_name)),
            timestamp: std::time::SystemTime::now(),
        };
        Ok(MotorFeedback::Completed(comp))
    }
}
