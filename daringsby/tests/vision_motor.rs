use daringsby::{vision_motor::VisionMotor, vision_sensor::VisionSensor};
use psyche_rs::{LLMClient, MotorError, SensorDirectingMotor};
use psyche_rs::llm::types::{Token, TokenStream};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;

struct DummyLLM;

#[async_trait::async_trait]
impl LLMClient for DummyLLM {
    async fn chat_stream(
        &self,
        _messages: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let stream = async_stream::stream! {
            yield Token { text: String::new() };
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
    let stream = Arc::new(VisionSensor::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = VisionMotor::new(stream, llm, tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["VisionSensor".to_string()]);
}

#[tokio::test]
async fn direct_sensor_valid_name_succeeds() {
    let stream = Arc::new(VisionSensor::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = VisionMotor::new(stream.clone(), llm, tx);
    SensorDirectingMotor::direct_sensor(&motor, "VisionSensor")
        .await
        .expect("should succeed");
}

#[tokio::test]
async fn direct_sensor_unknown_name_fails() {
    let stream = Arc::new(VisionSensor::default());
    let llm = Arc::new(DummyLLM);
    let (tx, _) = unbounded_channel();
    let motor = VisionMotor::new(stream.clone(), llm, tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}
