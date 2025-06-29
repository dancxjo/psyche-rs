use daringsby::source_tree_motor::SourceTreeMotor;
use futures::stream::{self, StreamExt};
use psyche_rs::{MotorError, SensorDirectingMotor};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn attached_sensors_returns_tree_sensor() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceTreeMotor::new(tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["SourceTreeSensor".to_string()]);
}

#[tokio::test]
async fn direct_sensor_emits_tree() {
    let (tx, mut rx) = unbounded_channel();
    let motor = SourceTreeMotor::new(tx);
    SensorDirectingMotor::direct_sensor(&motor, "SourceTreeSensor")
        .await
        .expect("should succeed");
    let sensations = rx.try_recv().expect("sensation sent");
    assert!(sensations[0].what.contains("daringsby"));
}

#[tokio::test]
async fn direct_sensor_unknown_name() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceTreeMotor::new(tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}

#[tokio::test]
async fn perform_returns_completion() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceTreeMotor::new(tx);
    use psyche_rs::{Action, Intention, Motor};
    let action = Action::new(
        "source_tree",
        serde_json::Value::Null,
        stream::empty().boxed(),
    );
    let intention = Intention::to(action).assign("source_tree");
    let result = motor.perform(intention).await.unwrap();
    assert!(result.completed);
    let completion = result.completion.unwrap();
    assert_eq!(completion.name, "source_tree");
    assert_eq!(completion.params, serde_json::Value::Null);
}
