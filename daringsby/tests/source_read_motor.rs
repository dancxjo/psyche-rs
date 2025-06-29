use daringsby::source_read_motor::SourceReadMotor;
use futures::stream::{self, StreamExt};
use psyche_rs::{MotorError, SensorDirectingMotor};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn attached_sensors_returns_block_sensor() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceReadMotor::new(tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["SourceBlockSensor".to_string()]);
}

#[tokio::test]
async fn direct_sensor_emits_block() {
    let (tx, mut rx) = unbounded_channel();
    let motor = SourceReadMotor::new(tx);
    // file path relative to daringsby crate
    SensorDirectingMotor::direct_sensor(&motor, "SourceBlockSensor:lib.rs:0")
        .await
        .expect("should succeed");
    let sensations = rx.try_recv().expect("sensation");
    assert!(sensations[0].what.contains("pub mod"));
}

#[tokio::test]
async fn direct_sensor_unknown() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceReadMotor::new(tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}

#[tokio::test]
async fn perform_returns_completion() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceReadMotor::new(tx);
    use psyche_rs::{Action, Intention, Motor};
    use serde_json::json;
    let params = json!({"file_path": "lib.rs", "block_index": 0});
    let action = Action::new("read_source", params, stream::empty().boxed());
    let intention = Intention::to(action).assign("read_source");
    let result = motor.perform(intention).await.expect("perform");
    assert!(result.completed);
    assert_eq!(result.completion.unwrap().name, "read_source");
}
