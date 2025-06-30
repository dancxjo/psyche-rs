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
        "Dedicate a string to your long term memory\n\
Parameters: none.\n\
Example:\n\
<log>System initialized successfully.</log>\n\
Explanation:\n\
The Will writes the body text to a persistent log file and stdout.\n\
The same text is emitted as a `log` sensation and can later be\
recalled via `read_log_memory`."
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
