use daringsby::source_search_motor::SourceSearchMotor;
use futures::stream::{self, StreamExt};
use psyche_rs::{MotorError, SensorDirectingMotor};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn attached_sensors_returns_search_sensor() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceSearchMotor::new(tx);
    let sensors = SensorDirectingMotor::attached_sensors(&motor);
    assert_eq!(sensors, vec!["SourceSearchSensor".to_string()]);
}

#[tokio::test]
async fn direct_sensor_emits_matches() {
    let (tx, mut rx) = unbounded_channel();
    let motor = SourceSearchMotor::new(tx);
    SensorDirectingMotor::direct_sensor(&motor, "SourceSearchSensor:LookMotor")
        .await
        .expect("should succeed");
    let sensations = rx.try_recv().expect("sensation");
    assert!(sensations[0].what.contains("LookMotor"));
}

#[tokio::test]
async fn direct_sensor_unknown_name() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceSearchMotor::new(tx);
    let err = SensorDirectingMotor::direct_sensor(&motor, "Unknown").await;
    assert!(matches!(err, Err(MotorError::Failed(_))));
}

#[tokio::test]
async fn perform_returns_completion() {
    let (tx, _rx) = unbounded_channel();
    let motor = SourceSearchMotor::new(tx);
    use psyche_rs::{Action, Intention, Motor};
    use serde_json::json;
    let params = json!({"query": "Motor"});
    let action = Action::new("search_source", params, stream::empty().boxed());
    let intention = Intention::to(action).assign("search_source");
    let result = motor.perform(intention).await.unwrap();
    assert!(result.completed);
    let completion = result.completion.unwrap();
    assert_eq!(completion.name, "search_source");
    assert_eq!(completion.params["query"], "Motor");
}
