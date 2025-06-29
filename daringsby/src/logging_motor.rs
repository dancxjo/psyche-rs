use tracing::{debug, info, warn};

use chrono::Local;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError, Sensation};

use crate::log_file::motor_log_path;

/// Simple motor that logs every received command.
#[derive(Default)]
pub struct LoggingMotor;

#[async_trait::async_trait]
impl Motor for LoggingMotor {
    fn description(&self) -> &'static str {
        "Log text by calling the `log` motor.\n\
Parameters: none. The body is written to a file and stdout.\n\
Associated sensor: `LogMemorySensor` via the `read_log_memory` motor\
to recall past entries.\n\
Emits a `log` sensation containing the text."
    }
    fn name(&self) -> &'static str {
        "log"
    }
    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        let mut action = intention.action;
        let text = action.collect_text().await;
        if text.trim().is_empty() {
            warn!(name = %action.name, "LoggingMotor received empty body");
        }
        info!(
            body = %text,
            name = %action.name,
            assigned_motor = %intention.assigned_motor,
            "motor log"
        );
        let line = format!(
            "{} - {}: {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            action.name,
            text
        );
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(motor_log_path())
            .await
        {
            let _ = file.write_all(line.as_bytes()).await;
        }
        let completion = Completion::of_action(action);
        debug!(
            completion_name = %completion.name,
            completion_params = ?completion.params,
            completion_result = ?completion.result,
            ?completion,
            "action completed"
        );
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "log".into(),
                when: Local::now(),
                what: serde_json::Value::String(text),
                source: None,
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}
