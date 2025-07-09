use daringsby::{vision_motor::VisionMotor, vision_sensor::VisionSensor};
use futures::{SinkExt, StreamExt, stream};
use psyche_rs::{LLMClient, MotorError, SensorDirectingMotor};
use psyche_rs::{Token, TokenStream};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

struct DummyLLM;

#[derive(Clone, Default)]
struct CaptureLLM {
    pub last: Arc<std::sync::Mutex<Vec<ollama_rs::generation::chat::ChatMessage>>>,
}

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

#[async_trait::async_trait]
impl LLMClient for CaptureLLM {
    async fn chat_stream(
        &self,
        msgs: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.last.lock().unwrap().extend_from_slice(msgs);
        Ok(Box::pin(stream::empty()))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![0.0])
    }
}

async fn start_server(stream: Arc<VisionSensor>) -> std::net::SocketAddr {
    let app = stream.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
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

#[tokio::test]
async fn prompt_warns_about_perspective() {
    let stream = Arc::new(VisionSensor::default());
    let addr = start_server(stream.clone()).await;
    let url = format!("ws://{addr}/vision-jpeg-in");
    let (mut ws, _) = connect_async(url).await.unwrap();
    let llm = Arc::new(CaptureLLM::default());
    let (tx, _rx) = unbounded_channel();
    let motor = VisionMotor::new(stream.clone(), llm.clone(), tx);
    use psyche_rs::{Action, Intention, Motor};
    use serde_json::Value;
    let motor_fut = tokio::spawn(async move {
        let body = stream::empty().boxed();
        let action = Action::new("look", Value::Null, body);
        let intent = Intention::to(action).assign("look");
        motor.perform(intent).await.unwrap();
    });
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg, WsMessage::Text("snap".into()));
    ws.send(WsMessage::Binary(vec![1, 2, 3])).await.unwrap();
    motor_fut.await.unwrap();
    let msgs = llm.last.lock().unwrap();
    assert!(msgs[0].content.contains("first-person perspective"));
}
