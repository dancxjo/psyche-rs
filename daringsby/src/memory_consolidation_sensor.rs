use async_stream::stream;
use chrono::{Local, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

/// Shared status for memory consolidation tasks.
#[derive(Default, Debug, Clone)]
pub struct ConsolidationStatus {
    pub in_progress: bool,
    pub last_finished: Option<chrono::DateTime<Utc>>,
    pub cluster_count: usize,
}

/// Sensor providing memory consolidation status snapshots.
pub struct MemoryConsolidationSensor {
    status: Arc<Mutex<ConsolidationStatus>>,
}

impl MemoryConsolidationSensor {
    /// Create a new sensor reading from the provided status.
    pub fn new(status: Arc<Mutex<ConsolidationStatus>>) -> Self {
        Self { status }
    }
}

impl Sensor<String> for MemoryConsolidationSensor {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let status = self.status.clone();
        Box::pin(stream! {
            let s = status.lock().await.clone();
            let msg = if s.in_progress {
                "Memory consolidation is running.".to_string()
            } else if let Some(finished) = s.last_finished {
                format!(
                    "Memory consolidation finished at {} with {} clusters.",
                    finished.to_rfc3339(),
                    s.cluster_count
                )
            } else {
                "Memory consolidation has not run yet.".to_string()
            };
            debug!(?msg, "memory consolidation status sensed");
            yield vec![Sensation {
                kind: "memory.consolidation.status".into(),
                when: Local::now(),
                what: msg,
                source: None,
            }];
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn emits_plain_english() {
        let status = ConsolidationStatus::default();
        let mut sensor = MemoryConsolidationSensor::new(Arc::new(Mutex::new(status)));
        let mut stream = sensor.stream();
        if let Some(batch) = stream.next().await {
            assert!(!batch[0].what.starts_with('{'));
        } else {
            panic!("no status emitted");
        }
    }
}
