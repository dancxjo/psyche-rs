use crate::memory::Intention;

#[async_trait::async_trait]
pub trait MotorSystem: Send + Sync {
    async fn invoke(&self, intention: &Intention) -> anyhow::Result<()>;
}

/// A basic motor implementation used for testing. It simply prints the intended
/// command in an XML-like format.
pub struct DummyMotor;

#[async_trait::async_trait]
impl MotorSystem for DummyMotor {
    async fn invoke(&self, intention: &Intention) -> anyhow::Result<()> {
        println!("<{} {:?}/>", intention.motor_name, intention.parameters);
        Ok(())
    }
}
