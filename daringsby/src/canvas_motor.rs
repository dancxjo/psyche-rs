use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Local;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, trace};

use psyche_rs::{ActionResult, Completion, Intention, LLMClient, Motor, MotorError, Sensation};

use crate::canvas_stream::CanvasStream;

/// Motor that captures a canvas snapshot and describes it using an LLM.
pub struct CanvasMotor {
    stream: Arc<CanvasStream>,
    llm: Arc<dyn LLMClient>,
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl CanvasMotor {
    /// Create a new motor backed by the given stream and LLM.
    pub fn new(
        stream: Arc<CanvasStream>,
        llm: Arc<dyn LLMClient>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        Self { stream, llm, tx }
    }
}

#[async_trait]
impl Motor for CanvasMotor {
    fn description(&self) -> &'static str {
        "Capture an image from the canvas and describe it"
    }

    fn name(&self) -> &'static str {
        "canvas"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        let action = intention.action;
        if action.name != "canvas" {
            return Err(MotorError::Unrecognized);
        }
        self.stream.request_snap();
        let mut rx = self.stream.subscribe();
        let img = rx
            .recv()
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let b64 = B64.encode(&img);
        let prompt = format!(
            "This is what you are seeing. If this is your first person perspective, what do you see?\n{b64}"
        );
        debug!(?prompt, "canvas prompt");
        let msgs = vec![ollama_rs::generation::chat::ChatMessage::user(prompt)];
        let mut stream = self
            .llm
            .chat_stream(&msgs)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let mut desc = String::new();
        while let Some(Ok(tok)) = stream.next().await {
            trace!(%tok, "llm token");
            desc.push_str(&tok);
        }
        let when = Local::now();
        let sensation = Sensation {
            kind: "vision.description".into(),
            when,
            what: desc.clone(),
            source: None,
        };
        let _ = self.tx.send(vec![sensation.clone()]);
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
                kind: "vision.description".into(),
                when,
                what: serde_json::Value::String(desc),
                source: None,
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[async_trait::async_trait]
impl psyche_rs::SensorDirectingMotor for CanvasMotor {
    /// Return the name of the single sensor controlled by this motor.
    fn attached_sensors(&self) -> Vec<String> {
        vec!["CanvasStream".to_string()]
    }

    /// Trigger a snapshot on the internal [`CanvasStream`].
    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name == "CanvasStream" {
            self.stream.request_snap();
            Ok(())
        } else {
            Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )))
        }
    }
}
