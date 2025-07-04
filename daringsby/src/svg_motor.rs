use async_trait::async_trait;
use chrono::Local;
use tokio::sync::mpsc::UnboundedSender;

use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError, Sensation};

/// Motor that broadcasts SVG drawings to connected clients.
///
/// # Example
/// ```
/// use daringsby::svg_motor::SvgMotor;
/// use futures::stream::{self, StreamExt};
/// use psyche_rs::{Action, Intention, Motor};
/// use tokio::sync::mpsc::unbounded_channel;
///
/// #[tokio::main]
/// async fn main() {
///     let (tx, mut rx) = unbounded_channel();
///     let motor = SvgMotor::new(tx);
///     let body = stream::iter(vec!["<svg/>".to_string()]).boxed();
///     let action = Action::new("draw", serde_json::Value::Null, body);
///     let intention = Intention::to(action).assign("draw");
///     motor.perform(intention).await.unwrap();
///     assert_eq!(rx.try_recv().unwrap(), "<svg/>");
/// }
/// ```
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
        "Envision something in your imagination using SVG markup.\n\
Parameters: none.\n\
Example:\n\
<draw><circle cx=\"5\" cy=\"5\" r=\"5\" fill=\"red\"/></draw>\n\
Explanation:\n\
The Will forwards the SVG markup to connected canvas clients for rendering.\n\
The same markup is emitted as a `drawing.svg` sensation for later reference."
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
