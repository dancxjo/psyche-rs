use futures::StreamExt;
use tracing::info;

use psyche_rs::{Action, ActionResult, Motor, MotorError};

/// Simple motor that logs every received command.
#[derive(Default)]
pub struct LoggingMotor;

#[async_trait::async_trait]
impl Motor for LoggingMotor {
    fn description(&self) -> &'static str {
        "Prints received actions to the log"
    }
    fn name(&self) -> &'static str {
        "log"
    }
    async fn perform(&self, mut action: Action) -> Result<ActionResult, MotorError> {
        let mut text = String::new();
        while let Some(chunk) = action.body.next().await {
            text.push_str(&chunk);
        }
        info!(body = %text, name = %action.intention.urge.name, "motor log");
        Ok(ActionResult {
            sensations: Vec::new(),
            completed: true,
        })
    }
}
