use async_trait::async_trait;
use chrono::Local;
use include_dir::{Dir, include_dir};
use tokio::sync::mpsc::UnboundedSender;

use psyche_rs::{
    ActionResult, Completion, Intention, Motor, MotorError, Sensation, SensorDirectingMotor,
};
use tracing::debug;

/// Embedded daringsby source directory.
static DARINGSBY_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src");
/// Embedded psyche-rs source directory.
static PSYCHE_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../psyche-rs/src");

/// Motor that reveals a tree view of Pete's source code.
///
/// The motor exposes a single sensor named `"SourceTreeSensor"` which, when
/// directed, sends one [`Sensation`] containing the directory tree of the
/// embedded source code.
///
/// # Examples
/// ```
/// use daringsby::source_tree_motor::SourceTreeMotor;
/// use psyche_rs::SensorDirectingMotor;
/// use tokio::sync::mpsc::unbounded_channel;
///
/// let (tx, mut rx) = unbounded_channel();
/// let motor = SourceTreeMotor::new(tx);
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// rt.block_on(async {
///     SensorDirectingMotor::direct_sensor(&motor, "SourceTreeSensor")
///         .await
///         .unwrap();
/// });
/// let sensations = rx.try_recv().unwrap();
/// assert!(sensations[0].what.contains("daringsby"));
/// ```
pub struct SourceTreeMotor {
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl SourceTreeMotor {
    /// Create a new motor sending sensations through the provided channel.
    pub fn new(tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        Self { tx }
    }

    fn gather(dir: &Dir, prefix: &str, lines: &mut Vec<String>) {
        for d in dir.dirs() {
            let name = d.path().file_name().unwrap().to_string_lossy();
            let sub = format!("{}/{}", prefix, name);
            lines.push(format!("{}/", sub));
            Self::gather(d, &sub, lines);
        }
        for f in dir.files() {
            let name = f.path().file_name().unwrap().to_string_lossy();
            lines.push(format!("{}/{}", prefix, name));
        }
    }

    fn tree() -> String {
        let mut lines = Vec::new();
        Self::gather(&DARINGSBY_SRC_DIR, "daringsby/src", &mut lines);
        Self::gather(&PSYCHE_SRC_DIR, "psyche-rs/src", &mut lines);
        lines.join("\n")
    }
}

#[async_trait]
impl Motor for SourceTreeMotor {
    fn description(&self) -> &'static str {
        "Use `source_tree` to list the project source tree.\n\
Parameters: none.\n\
Example:\n\
<source_tree></source_tree>\n\
Explanation:\n\
The Will gathers a directory listing of all embedded source files and returns\n\
it as a `source.tree` sensation. Directing `SourceTreeSensor` emits the same\n\
information."
    }

    fn name(&self) -> &'static str {
        "source_tree"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "source_tree" {
            return Err(MotorError::Unrecognized);
        }
        let tree = Self::tree();
        let completion = Completion::of_action(intention.action);
        debug!(
            completion_name = %completion.name,
            completion_params = ?completion.params,
            completion_result = ?completion.result,
            ?completion,
            "action completed"
        );
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "source.tree".into(),
                when: Local::now(),
                what: serde_json::Value::String(tree),
                source: None,
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[async_trait]
impl SensorDirectingMotor for SourceTreeMotor {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["SourceTreeSensor".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name != "SourceTreeSensor" {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let tree = Self::tree();
        let s = Sensation {
            kind: "source.tree".into(),
            when: Local::now(),
            what: tree,
            source: None,
        };
        let _ = self.tx.send(vec![s]);
        Ok(())
    }
}
