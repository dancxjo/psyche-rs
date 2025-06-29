use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::trace;

use psyche_rs::{Action, ActionResult, LLMClient, Motor, MotorError, Sensation};

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
        "Capture an image from the webcam and describe it"
    }

    fn name(&self) -> &'static str {
        "look"
    }

    async fn perform(&self, action: Action) -> Result<ActionResult, MotorError> {
        if action.intention.urge.name != "look" {
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
        trace!(?prompt, "look prompt");
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
        let sensation = Sensation {
            kind: "vision.description".into(),
            when: chrono::Local::now(),
            what: desc.clone(),
            source: None,
        };
        let _ = self.tx.send(vec![sensation.clone()]);
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "vision.description".into(),
                when: sensation.when,
                what: serde_json::Value::String(desc),
                source: None,
            }],
            completed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::{SinkExt, StreamExt, stream};
    use ollama_rs::generation::chat::ChatMessage;
    use psyche_rs::TokenStream;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

    async fn start_server(stream: Arc<LookStream>) -> std::net::SocketAddr {
        let app = stream.router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[derive(Clone)]
    struct StaticLLM;
    #[async_trait]
    impl LLMClient for StaticLLM {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Box::pin(stream::once(async { Ok("desc".to_string()) })))
        }
    }

    #[tokio::test]
    async fn performs_and_emits_sensation() {
        let stream = Arc::new(LookStream::default());
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/ws/look/in");
        let (mut ws, _) = connect_async(url).await.unwrap();
        tokio::spawn(async move {
            if let Some(Ok(WsMessage::Text(t))) = ws.next().await {
                if t == "snap" {
                    ws.send(WsMessage::Binary(vec![9])).await.unwrap();
                }
            }
        });
        let llm = Arc::new(StaticLLM);
        let (tx, mut rx) = unbounded_channel();
        let motor = LookMotor::new(stream.clone(), llm, tx);
        let action = Action::new("look", serde_json::Value::Null, stream::empty().boxed());
        let res = motor.perform(action).await.unwrap();
        assert!(res.completed);
        let batch = rx.recv().await.unwrap();
        assert_eq!(batch[0].what, "desc");
    }
}
