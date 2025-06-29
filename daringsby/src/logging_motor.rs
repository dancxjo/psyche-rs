use futures::StreamExt;
use tracing::{info, warn};

use chrono::Local;

use psyche_rs::{ActionResult, Intention, Motor, MotorError, Sensation};

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
    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        let mut action = intention.action;
        let mut text = String::new();
        while let Some(chunk) = action.body.next().await {
            text.push_str(&chunk);
        }
        if text.trim().is_empty() {
            warn!(name = %action.name, "LoggingMotor received empty body");
        }
        info!(
            body = %text,
            name = %action.name,
            assigned_motor = %intention.assigned_motor,
            "motor log"
        );
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "log".into(),
                when: Local::now(),
                what: serde_json::Value::String(text),
                source: None,
            }],
            completed: true,
            completion: None,
            interruption: None,
        })
    }
}
