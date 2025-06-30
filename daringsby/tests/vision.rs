use daringsby::{look_stream::LookStream, vision::Vision};
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
    let stream = Arc::new(LookStream::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = Vision::new(stream, llm, tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["LookStream".to_string()]);
}

#[tokio::test]
async fn direct_sensor_valid_name_succeeds() {
    let stream = Arc::new(LookStream::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = Vision::new(stream.clone(), llm, tx);
    SensorDirectingMotor::direct_sensor(&motor, "LookStream")
        .await
        .expect("should succeed");
}

#[tokio::test]
async fn direct_sensor_unknown_name_fails() {
    let stream = Arc::new(LookStream::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = Vision::new(stream.clone(), llm, tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}
