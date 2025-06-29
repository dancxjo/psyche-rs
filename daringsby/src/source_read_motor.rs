use async_trait::async_trait;
use chrono::Local;
use include_dir::{Dir, include_dir};
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;

use psyche_rs::{
    ActionResult, Completion, Intention, Motor, MotorError, Sensation, SensorDirectingMotor,
};

static DARINGSBY_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src");
static PSYCHE_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../psyche-rs/src");

const MAX_LINES: usize = 20;

/// Motor that reads blocks of source code on demand.
///
/// The sensor `"SourceBlockSensor"` emits a single [`Sensation`] with the
/// requested portion of a file when directed. Parameters are supplied by
/// appending `:"path":index` to the sensor name, e.g. `"SourceBlockSensor:src/main.rs:1"`.
/// The index selects the `MAX_LINES` sized chunk to return.
pub struct SourceReadMotor {
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl SourceReadMotor {
    /// Create a new motor sending sensations through the provided channel.
    pub fn new(tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        Self { tx }
    }

    fn get_file(path: &str) -> Option<&'static include_dir::File<'static>> {
        DARINGSBY_SRC_DIR
            .get_file(path)
            .or_else(|| PSYCHE_SRC_DIR.get_file(path))
    }

    fn read_block(path: &str, index: usize) -> Result<String, MotorError> {
        let file = Self::get_file(path)
            .ok_or_else(|| MotorError::Failed(format!("unknown file: {}", path)))?;
        let text = file
            .contents_utf8()
            .ok_or_else(|| MotorError::Failed("invalid utf8".into()))?;
        let lines: Vec<&str> = text.lines().collect();
        let chunks: Vec<&[&str]> = lines.chunks(MAX_LINES).collect();
        let idx = index.min(chunks.len().saturating_sub(1));
        Ok(chunks[idx].join("\n"))
    }
}

#[async_trait]
impl Motor for SourceReadMotor {
    fn description(&self) -> &'static str {
        "Read a block of source code"
    }

    fn name(&self) -> &'static str {
        "read_source"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "read_source" {
            return Err(MotorError::Unrecognized);
        }
        let action = intention.action;
        let path = action
            .params
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| MotorError::Failed("missing file_path".into()))?;
        let index = action
            .params
            .get("block_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let block = Self::read_block(&path, index)?;
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
                kind: "source.block".into(),
                when: Local::now(),
                what: serde_json::Value::String(block),
                source: Some(path.clone()),
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[async_trait]
impl SensorDirectingMotor for SourceReadMotor {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["SourceBlockSensor".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if !sensor_name.starts_with("SourceBlockSensor") {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let mut parts = sensor_name.splitn(3, ':');
        let _ = parts.next();
        let path = parts
            .next()
            .ok_or_else(|| MotorError::Failed("missing file path".into()))?;
        let index = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let block = Self::read_block(path, index)?;
        let s = Sensation {
            kind: "source.block".into(),
            when: Local::now(),
            what: block,
            source: Some(path.to_string()),
        };
        let _ = self.tx.send(vec![s]);
        Ok(())
    }
}
