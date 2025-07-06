use async_trait::async_trait;
use chrono::{Local, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc::UnboundedSender};
use tracing::{debug, info, warn};

use psyche_rs::{
    ActionResult, ClusterAnalyzer, Completion, Intention, LLMClient, MemoryStore, Motor,
    MotorError, Sensation,
};

use crate::memory_consolidation_sensor::ConsolidationStatus;

fn delay() -> std::time::Duration {
    if std::env::var("FAST_TEST").is_ok() {
        std::time::Duration::from_millis(50)
    } else {
        std::time::Duration::from_secs(0)
    }
}

/// Motor that consolidates memory summaries using [`ClusterAnalyzer`].
///
/// The action tag `<consolidate></consolidate>` triggers consolidation.
/// Summaries are created for recent impressions in small groups. The process
/// runs asynchronously and its progress is reflected in
/// [`ConsolidationStatus`].
pub struct MemoryConsolidationMotor<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> {
    analyzer: Arc<ClusterAnalyzer<M, C>>,
    status: Arc<Mutex<ConsolidationStatus>>,
    batch_size: usize,
    cluster_size: usize,
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl<M: MemoryStore + Send + Sync, C: LLMClient + ?Sized> MemoryConsolidationMotor<M, C> {
    /// Create a new motor backed by the given analyzer and status holder.
    pub fn new(
        analyzer: Arc<ClusterAnalyzer<M, C>>,
        status: Arc<Mutex<ConsolidationStatus>>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        Self {
            analyzer,
            status,
            batch_size: 20,
            cluster_size: 5,
            tx,
        }
    }

    async fn run_consolidation(
        analyzer: Arc<ClusterAnalyzer<M, C>>,
        status: Arc<Mutex<ConsolidationStatus>>,
        batch_size: usize,
        cluster_size: usize,
    ) {
        {
            let mut s = status.lock().await;
            s.in_progress = true;
        }
        tokio::time::sleep(delay()).await;
        let imps = match analyzer.store.fetch_recent_impressions(batch_size).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error=?e, "failed to fetch impressions");
                let mut s = status.lock().await;
                s.in_progress = false;
                return;
            }
        };
        let clusters: Vec<Vec<String>> = imps
            .chunks(cluster_size)
            .map(|c| c.iter().map(|i| i.id.clone()).collect())
            .collect();
        let count = clusters.len();
        if let Err(e) = analyzer.summarize(clusters).await {
            warn!(error=?e, "consolidation summarize failed");
        }
        {
            let mut s = status.lock().await;
            s.in_progress = false;
            s.last_finished = Some(Utc::now());
            s.cluster_count = count;
        }
        info!("memory consolidation complete");
    }
}

#[async_trait]
impl<M: MemoryStore + Send + Sync + 'static, C: LLMClient + ?Sized + 'static> Motor
    for MemoryConsolidationMotor<M, C>
{
    fn description(&self) -> &'static str {
        "Consolidate related memories into summaries.\n\
Parameters: none.\n\
Example:\n\
<consolidate></consolidate>"
    }

    fn name(&self) -> &'static str {
        "consolidate"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "consolidate" {
            return Err(MotorError::Unrecognized);
        }
        let analyzer = self.analyzer.clone();
        let status = self.status.clone();
        let batch = self.batch_size;
        let cluster = self.cluster_size;
        tokio::spawn(Self::run_consolidation(analyzer, status, batch, cluster));
        let completion = Completion::of_action(intention.action);
        debug!(?completion, "action completed");
        let started = Sensation {
            kind: "memory.consolidation.started".into(),
            when: Local::now(),
            what: "started".to_string(),
            source: None,
        };
        let _ = self.tx.send(vec![started.clone()]);
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "memory.consolidation.started".into(),
                when: Local::now(),
                what: serde_json::Value::String("started".into()),
                source: None,
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}
