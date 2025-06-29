use daringsby::logging_motor::LoggingMotor;
use futures::{StreamExt, stream};
use psyche_rs::{Action, Intention, Motor};

#[tokio::test]
async fn perform_accepts_body_and_succeeds() {
    let motor = LoggingMotor::default();
    let body = stream::iter(vec!["hello".to_string(), " world".to_string()]).boxed();
    let action = Action::new("log", serde_json::Value::Null, body);
    let intention = Intention::to(action).assign("log");
    let result = motor
        .perform(intention)
        .await
        .expect("perform should succeed");
    assert!(result.completed);
    let completion = result.completion.expect("completion");
    assert_eq!(completion.name, "log");
    assert_eq!(completion.params, serde_json::Value::Null);
    assert!(result.interruption.is_none());
    assert_eq!(result.sensations.len(), 1);
    assert_eq!(result.sensations[0].what, "hello world");
}
