use async_trait::async_trait;
use chrono::Local;
use tokio::sync::mpsc::UnboundedSender;

use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError, Sensation};

/// Motor that broadcasts SVG drawings to connected clients.
pub struct SvgMotor {
    tx: UnboundedSender<String>,
}

impl SvgMotor {
    /// Create a new SvgMotor backed by the provided channel.
    pub fn new(tx: UnboundedSender<String>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl Motor for SvgMotor {
    fn description(&self) -> &'static str {
        "Broadcast SVG drawings to canvas clients"
    }

    fn name(&self) -> &'static str {
        "draw"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        let mut action = intention.action;
        if action.name != "draw" {
            return Err(MotorError::Unrecognized);
        }
        let svg = action.collect_text().await;
        let _ = self.tx.send(svg.clone());
        let when = Local::now();
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "drawing.svg".into(),
                when,
                what: serde_json::Value::String(svg),
                source: None,
            }],
            completed: true,
            completion: Some(Completion::of_action(action)),
            interruption: None,
        })
    }
}
