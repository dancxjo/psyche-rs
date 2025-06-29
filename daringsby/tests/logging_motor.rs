use daringsby::logging_motor::LoggingMotor;
use futures::{StreamExt, stream};
use psyche_rs::{Action, Intention, Motor, Urge};

#[tokio::test]
async fn perform_accepts_body_and_succeeds() {
    let motor = LoggingMotor::default();
    let body = stream::iter(vec!["hello".to_string(), " world".to_string()]).boxed();
    let intention = Intention::assign(Urge::new("log"), "log");
    let action = Action { intention, body };
    let result = motor.perform(action).await.expect("perform should succeed");
    assert!(result.completed);
    assert_eq!(result.sensations.len(), 1);
    assert_eq!(result.sensations[0].what, "hello world");
}
