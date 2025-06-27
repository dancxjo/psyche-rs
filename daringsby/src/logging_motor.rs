use async_trait::async_trait;
use tracing::info;

use psyche_rs::{Motor, MotorCommand};

/// Simple motor that logs every received command.
#[derive(Default)]
pub struct LoggingMotor;

#[async_trait(?Send)]
impl Motor<String> for LoggingMotor {
    async fn execute(&mut self, command: MotorCommand<String>) {
        info!(?command.content, "motor log");
    }
}
