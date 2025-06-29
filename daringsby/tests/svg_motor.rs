use daringsby::svg_motor::SvgMotor;
use futures::stream::{self, StreamExt};
use psyche_rs::{Action, Intention, Motor};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn perform_sends_svg_and_returns_sensation() {
    let (tx, mut rx) = unbounded_channel();
    let motor = SvgMotor::new(tx);
    let body = stream::iter(vec!["<svg/>".to_string()]).boxed();
    let action = Action::new("draw", serde_json::Value::Null, body);
    let intention = Intention::to(action).assign("draw");
    let result = motor.perform(intention).await.expect("perform");
    assert!(result.completed);
    assert_eq!(result.sensations[0].kind, "drawing.svg");
    let sent = rx.try_recv().expect("svg broadcast");
    assert_eq!(sent, "<svg/>");
}
