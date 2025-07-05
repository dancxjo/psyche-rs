use daringsby::{canvas_motor::CanvasMotor, canvas_stream::CanvasStream};
use futures::stream::{self, StreamExt};
use psyche_rs::{LLMClient, Motor, MotorError, SensorDirectingMotor};
use psyche_rs::{Token, TokenStream};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;

struct DummyLLM(&'static str);

#[async_trait::async_trait]
impl LLMClient for DummyLLM {
    async fn chat_stream(
        &self,
        _messages: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let reply = self.0.to_string();
        let stream = async_stream::stream! {
            yield Token { text: reply };
        };
        Ok(Box::pin(stream))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![0.0])
    }
}

#[tokio::test]
async fn attached_sensors_returns_stream_name() {
    let stream = Arc::new(CanvasStream::default());
    let llm = Arc::new(DummyLLM(""));
    let (tx, _) = unbounded_channel();
    let motor = CanvasMotor::new(stream, llm, tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["CanvasStream".to_string()]);
}

#[tokio::test]
async fn direct_sensor_triggers_snapshot() {
    let stream = Arc::new(CanvasStream::default());
    let mut cmd_rx = stream.subscribe_cmd();
    let llm = Arc::new(DummyLLM(""));
    let (tx, _) = unbounded_channel();
    let motor = CanvasMotor::new(stream.clone(), llm, tx);
    SensorDirectingMotor::direct_sensor(&motor, "CanvasStream")
        .await
        .expect("should succeed");
    // command should have been sent
    let cmd = cmd_rx.recv().await.unwrap();
    assert_eq!(cmd, "snap");
}

#[tokio::test]
async fn direct_sensor_unknown_name_fails() {
    let stream = Arc::new(CanvasStream::default());
    let llm = Arc::new(DummyLLM(""));
    let (tx, _) = unbounded_channel();
    let motor = CanvasMotor::new(stream, llm, tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}

#[tokio::test]
async fn perform_emits_description_and_broadcasts() {
    let stream = Arc::new(CanvasStream::default());
    let llm = Arc::new(DummyLLM("sky "));
    let (tx, mut rx) = unbounded_channel();
    let motor = CanvasMotor::new(stream.clone(), llm, tx);

    let mut cmd_rx = stream.subscribe_cmd();
    let s = stream.clone();
    tokio::spawn(async move {
        if cmd_rx.recv().await.is_ok() {
            s.push_image(vec![1, 2, 3]);
        }
    });

    let body = stream::empty().boxed();
    let action = psyche_rs::Action::new("canvas", serde_json::Value::Null, body);
    let intention = psyche_rs::Intention::to(action).assign("canvas");
    let result = motor.perform(intention).await.expect("perform");
    assert!(result.completed);
    assert_eq!(result.sensations[0].kind, "vision.description");
    let batch = rx.try_recv().expect("broadcasted");
    assert_eq!(batch[0].what, "sky ");
}
