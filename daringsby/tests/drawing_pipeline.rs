use daringsby::{canvas_stream::CanvasStream, svg_motor::SvgMotor};
use futures::stream::{self, StreamExt};
use psyche_rs::Motor;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn svg_is_broadcast_to_canvas() {
    let canvas = Arc::new(CanvasStream::default());
    let mut canvas_rx = canvas.subscribe_svg();

    let (tx, mut rx) = unbounded_channel();
    let motor = SvgMotor::new(tx);
    tokio::spawn({
        let c = canvas.clone();
        async move {
            while let Some(svg) = rx.recv().await {
                c.broadcast_svg(svg);
            }
        }
    });

    let body = stream::iter(vec!["<svg><rect/></svg>".to_string()]).boxed();
    let action = psyche_rs::Action::new("draw", serde_json::Value::Null, body);
    let intention = psyche_rs::Intention::to(action).assign("draw");
    motor.perform(intention).await.unwrap();

    let received = canvas_rx.recv().await.unwrap();
    assert!(received.contains("rect"));
}
