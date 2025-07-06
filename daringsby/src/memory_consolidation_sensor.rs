use async_stream::stream;
use chrono::{Local, Utc};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

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
            let data = json!({
                "in_progress": s.in_progress,
                "last_finished": s.last_finished.map(|d| d.to_rfc3339()),
                "cluster_count": s.cluster_count,
            });
            yield vec![Sensation {
                kind: "memory.consolidation.status".into(),
                when: Local::now(),
                what: data.to_string(),
                source: None,
            }];
        })
    }
}
