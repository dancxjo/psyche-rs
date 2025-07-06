use crate::BatteryMotor;
use crate::{
    CanvasMotor, LogMemoryMotor, LoggingMotor, Mouth, RecallMotor, SourceReadMotor,
    SourceSearchMotor, SourceTreeMotor, SvgMotor, VisionMotor,
};
use futures::StreamExt;
use psyche_rs::Sensation;
use psyche_rs::{Impression, Intention, Motor};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn drive_combo_stream(
    mut combo_stream: impl futures::Stream<Item = Vec<Impression<Impression<String>>>>
    + Unpin
    + Send
    + 'static,
    _logger: Arc<LoggingMotor>,
    sens_tx: tokio::sync::mpsc::UnboundedSender<Vec<Sensation<String>>>,
    moment: Arc<Mutex<Vec<Impression<Impression<String>>>>>,
) {
    while let Some(_imps) = combo_stream.next().await {
        let mut guard = moment.lock().await;
        *guard = _imps.clone();
        drop(guard);
        let sensed: Vec<Sensation<String>> = _imps
            .iter()
            .map(|imp| Sensation {
                kind: "impression".into(),
                when: chrono::Local::now(),
                what: imp.how.clone(),
                source: None,
            })
            .collect();
        let _ = sens_tx.send(sensed);
    }
}

pub async fn drive_will_stream<M>(
    mut will_stream: impl futures::Stream<Item = Vec<Intention>> + Unpin + Send + 'static,
    logger: Arc<LoggingMotor>,
    vision_motor: Arc<VisionMotor>,
    mouth: Arc<Mouth>,
    canvas: Arc<CanvasMotor>,
    drawer: Arc<SvgMotor>,
    recall: Arc<RecallMotor<M>>,
    log_memory: Arc<LogMemoryMotor>,
    source_read: Arc<SourceReadMotor>,
    source_search: Arc<SourceSearchMotor>,
    source_tree: Arc<SourceTreeMotor>,
    battery: Arc<BatteryMotor>,
) where
    M: psyche_rs::MemoryStore + Send + Sync + 'static,
{
    while let Some(ints) = will_stream.next().await {
        for intent in ints {
            match intent.assigned_motor.as_str() {
                "log" => {
                    logger.perform(intent).await.expect("logging motor failed");
                }
                "look" => {
                    vision_motor
                        .perform(intent)
                        .await
                        .expect("look motor failed");
                }
                "speak" => {
                    mouth.perform(intent).await.expect("mouth motor failed");
                }
                "canvas" => {
                    canvas.perform(intent).await.expect("canvas motor failed");
                }
                "draw" => {
                    drawer.perform(intent).await.expect("svg motor failed");
                }
                "recall" => {
                    recall.perform(intent).await.expect("recall motor failed");
                }
                "read_log_memory" => {
                    log_memory
                        .perform(intent)
                        .await
                        .expect("log memory motor failed");
                }
                "read_source" => {
                    source_read
                        .perform(intent)
                        .await
                        .expect("read source motor failed");
                }
                "search_source" => {
                    source_search
                        .perform(intent)
                        .await
                        .expect("search source motor failed");
                }
                "source_tree" => {
                    source_tree
                        .perform(intent)
                        .await
                        .expect("source tree motor failed");
                }
                "battery" => {
                    battery.perform(intent).await.expect("battery motor failed");
                }
                _ => {
                    tracing::warn!(motor = %intent.assigned_motor, "unknown motor");
                }
            }
        }
    }
}
