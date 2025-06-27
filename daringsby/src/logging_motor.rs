use futures::StreamExt;
use tracing::info;

use psyche_rs::{Action, Motor, MotorError};

/// Simple motor that logs every received command.
#[derive(Default)]
pub struct LoggingMotor;

impl Motor for LoggingMotor {
    fn description(&self) -> &'static str {
        "Prints received actions to the log"
    }
    fn perform(&self, mut action: Action) -> Result<(), MotorError> {
        futures::executor::block_on(async {
            let mut text = String::new();
            while let Some(chunk) = action.body.next().await {
                text.push_str(&chunk);
            }
            info!(body = %text, name = %action.name, "motor log");
        });
        Ok(())
    }
}
