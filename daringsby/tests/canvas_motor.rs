use daringsby::{canvas_motor::CanvasMotor, canvas_stream::CanvasStream};
use psyche_rs::{LLMClient, MotorError, SensorDirectingMotor};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;

struct DummyLLM;

#[async_trait::async_trait]
impl LLMClient for DummyLLM {
    async fn chat_stream(
        &self,
        _messages: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<psyche_rs::LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let stream = async_stream::stream! {
            yield Ok(String::new());
        };
        Ok(Box::pin(stream))
    }
}

#[tokio::test]
async fn attached_sensors_returns_stream_name() {
    let stream = Arc::new(CanvasStream::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = CanvasMotor::new(stream, llm, tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["CanvasStream".to_string()]);
}

#[tokio::test]
async fn direct_sensor_triggers_snapshot() {
    let stream = Arc::new(CanvasStream::default());
    let mut cmd_rx = stream.subscribe_cmd();
    let llm = Arc::new(DummyLLM);
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
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = CanvasMotor::new(stream, llm, tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}
