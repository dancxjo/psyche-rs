use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Local;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, trace};

use psyche_rs::{ActionResult, Completion, Intention, LLMClient, Motor, MotorError, Sensation};

use crate::look_stream::LookStream;

/// Motor that captures a webcam snapshot and describes it using an LLM.
pub struct LookMotor {
    stream: Arc<LookStream>,
    llm: Arc<dyn LLMClient>,
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl LookMotor {
    /// Create a new look motor backed by the given stream and LLM.
    pub fn new(
        stream: Arc<LookStream>,
        llm: Arc<dyn LLMClient>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        Self { stream, llm, tx }
    }
}

#[async_trait]
impl Motor for LookMotor {
    fn description(&self) -> &'static str {
        "Take a look at what's in front of your face.\n\
Parameters: none.\n\
Example:\n\
<look></look>\n\
Explanation:\n\
The Will triggers a webcam snapshot and asks the LLM to describe the image.\n\
The resulting description is returned as a `vision.description` sensation and\
sent to any `LookStream` subscribers."
    }

    fn name(&self) -> &'static str {
        "look"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        let action = intention.action;
        if action.name != "look" {
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
        debug!(?prompt, "look prompt");
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
impl psyche_rs::SensorDirectingMotor for LookMotor {
    /// Return the name of the single sensor controlled by this motor.
    fn attached_sensors(&self) -> Vec<String> {
        vec!["LookStream".to_string()]
    }

    /// Trigger a snapshot on the internal [`LookStream`].
    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name == "LookStream" {
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
