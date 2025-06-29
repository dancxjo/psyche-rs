use async_trait::async_trait;
use chrono::Local;
use tokio::{fs, io::AsyncReadExt, sync::mpsc::UnboundedSender};
use tracing::debug;

use psyche_rs::{
    ActionResult, Completion, Intention, Motor, MotorError, Sensation, SensorDirectingMotor,
};

use crate::log_file::motor_log_path;

/// Motor that reads Pete's motor log memory.
pub struct LogMemoryMotor {
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl LogMemoryMotor {
    /// Create a new motor sending sensations through the provided channel.
    pub fn new(tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        Self { tx }
    }

    async fn read_log() -> Result<String, MotorError> {
        let path = motor_log_path();
        let mut f = fs::File::open(&path)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let mut contents = String::new();
        f.read_to_string(&mut contents)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        Ok(contents)
    }
}

#[async_trait]
impl Motor for LogMemoryMotor {
    fn description(&self) -> &'static str {
        "Read Pete's motor log memory"
    }

    fn name(&self) -> &'static str {
        "read_log_memory"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "read_log_memory" {
            return Err(MotorError::Unrecognized);
        }
        let log = Self::read_log().await?;
        let completion = Completion::of_action(intention.action);
        debug!(?completion, "action completed");
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "log.memory".into(),
                when: Local::now(),
                what: serde_json::Value::String(log),
                source: Some(motor_log_path().display().to_string()),
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[async_trait]
impl SensorDirectingMotor for LogMemoryMotor {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["LogMemorySensor".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name != "LogMemorySensor" {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let log = Self::read_log().await?;
        let s = Sensation {
            kind: "log.memory".into(),
            when: Local::now(),
            what: log,
            source: Some(motor_log_path().display().to_string()),
        };
        let _ = self.tx.send(vec![s]);
        Ok(())
    }
}
