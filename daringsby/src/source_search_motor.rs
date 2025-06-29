use async_trait::async_trait;
use chrono::Local;
use include_dir::{Dir, include_dir};
use tokio::sync::mpsc::UnboundedSender;

use psyche_rs::{
    ActionResult, Completion, Intention, Motor, MotorError, Sensation, SensorDirectingMotor,
};
use tracing::debug;

static DARINGSBY_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src");
static PSYCHE_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../psyche-rs/src");

/// Motor that searches source files for a query string.
///
/// The `"SourceSearchSensor"` emits a [`Sensation`] for each matching line when
/// directed. Parameters are appended to the sensor name using
/// `:"query"`, for example `"SourceSearchSensor:LookMotor"`.
pub struct SourceSearchMotor {
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl SourceSearchMotor {
    /// Create a new motor sending sensations through the provided channel.
    pub fn new(tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        Self { tx }
    }

    fn search(query: &str) -> Vec<Sensation<String>> {
        let mut out = Vec::new();
        for (prefix, dir) in [
            ("daringsby/src", &DARINGSBY_SRC_DIR),
            ("psyche-rs/src", &PSYCHE_SRC_DIR),
        ] {
            for file in dir.files() {
                if file.path().extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                if let Some(text) = file.contents_utf8() {
                    for (i, line) in text.lines().enumerate() {
                        if line.contains(query) {
                            let what = format!(
                                "{}:{}: {}",
                                format!("{}/{}", prefix, file.path().display()),
                                i + 1,
                                line.trim_end()
                            );
                            out.push(Sensation {
                                kind: "source.search".into(),
                                when: Local::now(),
                                what,
                                source: None,
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

#[async_trait]
impl Motor for SourceSearchMotor {
    fn description(&self) -> &'static str {
        "Search the source code for a string"
    }

    fn name(&self) -> &'static str {
        "search_source"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "search_source" {
            return Err(MotorError::Unrecognized);
        }
        let action = intention.action;
        let query = action
            .params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MotorError::Failed("missing query".into()))?;
        let results = Self::search(query);
        let completion = Completion::of_action(action);
        debug!(
            completion_name = %completion.name,
            completion_params = ?completion.params,
            completion_result = ?completion.result,
            ?completion,
            "action completed"
        );
        Ok(ActionResult {
            sensations: results
                .into_iter()
                .map(|s| Sensation {
                    kind: s.kind,
                    when: s.when,
                    what: serde_json::Value::String(s.what),
                    source: s.source,
                })
                .collect(),
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[async_trait]
impl SensorDirectingMotor for SourceSearchMotor {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["SourceSearchSensor".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if !sensor_name.starts_with("SourceSearchSensor") {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let mut parts = sensor_name.splitn(2, ':');
        let _ = parts.next();
        let query = parts
            .next()
            .ok_or_else(|| MotorError::Failed("missing query".into()))?;
        let results = Self::search(query);
        if !results.is_empty() {
            let _ = self.tx.send(results);
        }
        Ok(())
    }
}
