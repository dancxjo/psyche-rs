use psyche_rs::memory::{Intention, IntentionStatus};
use psyche_rs::motor::{DummyMotor, Motor, MotorEvent};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test]
async fn motor_receives_event_stream() {
    let motor = DummyMotor::new();
    let log = motor.log.clone();
    let (tx, rx) = mpsc::channel(8);
    let motor_clone = motor.clone();
    tokio::spawn(async move {
        motor_clone.handle(rx).await.unwrap();
    });

    let intention = Intention {
        uuid: Uuid::new_v4(),
        urge: Uuid::new_v4(),
        motor_name: "doit".into(),
        parameters: json!({}),
        issued_at: std::time::SystemTime::now(),
        resolved_at: None,
        status: IntentionStatus::Pending,
    };

    tx.send(MotorEvent::Begin(intention.clone())).await.unwrap();
    tx.send(MotorEvent::Chunk("a".into())).await.unwrap();
    tx.send(MotorEvent::Chunk("b".into())).await.unwrap();
    tx.send(MotorEvent::End).await.unwrap();
    drop(tx);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let events = log.lock().await.clone();
    assert_eq!(events.len(), 4);
    matches!(events[0], MotorEvent::Begin(_));
    assert_eq!(events[1], MotorEvent::Chunk("a".into()));
    assert_eq!(events[2], MotorEvent::Chunk("b".into()));
    assert_eq!(events[3], MotorEvent::End);
}
