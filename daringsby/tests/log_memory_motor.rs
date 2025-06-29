use daringsby::log_memory_motor::LogMemoryMotor;
use daringsby::logging_motor::LoggingMotor;
use futures::{StreamExt, stream};
use psyche_rs::{Action, Intention, Motor, SensorDirectingMotor};
use serial_test::serial;
use tempfile::NamedTempFile;
use tokio::sync::mpsc::unbounded_channel;

fn set_log_path(file: &NamedTempFile) {
    unsafe { std::env::set_var("DARINGSBY_MOTOR_LOG_PATH", file.path()) };
}

#[tokio::test]
#[serial]
async fn direct_sensor_reads_log() {
    let tmp = NamedTempFile::new().unwrap();
    set_log_path(&tmp);
    tokio::fs::write(tmp.path(), "first line").await.unwrap();
    let (tx, mut rx) = unbounded_channel();
    let motor = LogMemoryMotor::new(tx);
    SensorDirectingMotor::direct_sensor(&motor, "LogMemorySensor")
        .await
        .unwrap();
    let batch = rx.try_recv().unwrap();
    assert_eq!(batch[0].what, "first line");
}

#[tokio::test]
#[serial]
async fn perform_returns_log() {
    let tmp = NamedTempFile::new().unwrap();
    set_log_path(&tmp);
    let log_motor = LoggingMotor::default();
    let body = stream::iter(vec!["hi".to_string()]).boxed();
    let action = Action::new("log", serde_json::Value::Null, body);
    let intention = Intention::to(action).assign("log");
    Motor::perform(&log_motor, intention).await.unwrap();

    let (tx, _rx) = unbounded_channel();
    let reader = LogMemoryMotor::new(tx);
    let body = stream::iter(vec![]).boxed();
    let action = Action::new("read_log_memory", serde_json::Value::Null, body);
    let intention = Intention::to(action).assign("read_log_memory");
    let result = reader.perform(intention).await.unwrap();
    let log = result.sensations[0].what.as_str().expect("string log");
    assert!(log.contains("hi"));
}
