use async_trait::async_trait;
use chrono::{Local, Utc};
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, trace};
use uuid::Uuid;

use psyche_rs::{
    ActionResult, Completion, Intention, LLMClient, MemoryStore, Motor, MotorError, Sensation,
    SensorDirectingMotor, StoredImpression,
};

/// Motor that discovers neighbors of recent impressions and summarizes them.
///
/// The associated sensor emits a `neighbor.summary` sensation containing the
/// one-sentence summary.
pub struct NeighborDiscoveryMotor<M: MemoryStore> {
    store: M,
    llm: Arc<dyn LLMClient>,
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl<M: MemoryStore> NeighborDiscoveryMotor<M> {
    /// Create a new motor backed by the given store and LLM.
    pub fn new(
        store: M,
        llm: Arc<dyn LLMClient>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
    ) -> Self {
        Self { store, llm, tx }
    }

    async fn discover_and_summarize(&self) -> Result<String, MotorError> {
        let recents = self
            .store
            .fetch_recent_impressions(1)
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let moment = match recents.into_iter().next() {
            Some(m) => m,
            None => return Err(MotorError::Failed("no recent impressions".into())),
        };
        let neighbors = self
            .store
            .retrieve_related_impressions(&moment.how, 3)
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let ctx = json!({
            "moment": moment.how,
            "neighbors": neighbors.iter().map(|n| n.how.clone()).collect::<Vec<_>>()
        });
        let prompt = format!(
            "These are the recent moment and its neighbors:\n{}\nSummarize what is relevant from the neighbors for this moment in one sentence.",
            ctx
        );
        trace!(%prompt, "neighbor_discovery_prompt");
        let msgs = [ollama_rs::generation::chat::ChatMessage::user(prompt)];
        let mut stream = self
            .llm
            .chat_stream(&msgs)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let mut out = String::new();
        while let Some(tok) = stream.next().await {
            let tok = tok.map_err(|e| MotorError::Failed(e.to_string()))?;
            trace!(%tok, "neighbor_llm_token");
            out.push_str(&tok);
        }
        let summary = out.trim();
        if summary.is_empty() {
            return Err(MotorError::Failed("empty summary".into()));
        }
        let stored = StoredImpression {
            id: Uuid::new_v4().to_string(),
            kind: "neighbor.summary".into(),
            when: Utc::now(),
            how: summary.to_string(),
            sensation_ids: Vec::new(),
            impression_ids: vec![moment.id],
        };
        self.store
            .store_summary_impression(&stored, &stored.impression_ids)
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let s = Sensation {
            kind: "neighbor.summary".into(),
            when: Local::now(),
            what: summary.to_string(),
            source: None,
        };
        let _ = self.tx.send(vec![s]);
        Ok(summary.to_string())
    }
}

#[async_trait]
impl<M: MemoryStore + Send + Sync> Motor for NeighborDiscoveryMotor<M> {
    fn description(&self) -> &'static str {
        "Discovers nearest neighbors of recent moments and summarizes them.\n\
Example:\n\
<discover_neighbors></discover_neighbors>\n\
Queries for neighbors, summarizes relevant context, emits as an impression."
    }

    fn name(&self) -> &'static str {
        "discover_neighbors"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "discover_neighbors" {
            return Err(MotorError::Unrecognized);
        }
        let action = intention.action;
        match self.discover_and_summarize().await {
            Ok(_) => {
                let completion = Completion::of_action(action);
                debug!(?completion, "action completed");
                Ok(ActionResult {
                    sensations: Vec::new(),
                    completed: true,
                    completion: Some(completion),
                    interruption: None,
                })
            }
            Err(e) => {
                error!(?e, "neighbor discovery failed");
                Err(e)
            }
        }
    }
}

#[async_trait]
impl<M: MemoryStore + Send + Sync> SensorDirectingMotor for NeighborDiscoveryMotor<M> {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["NeighborSummarySensor".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name != "NeighborSummarySensor" {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let _ = self.discover_and_summarize().await?;
        Ok(())
    }
}
