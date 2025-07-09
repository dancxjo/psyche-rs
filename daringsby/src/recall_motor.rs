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

/// Motor that recalls impressions related to a provided sentence and
/// summarizes them.
///
/// The associated sensor emits a `neighbor.summary` sensation containing the
/// one-sentence summary.
pub struct RecallMotor<M: MemoryStore + Send + Sync> {
    store: M,
    llm: Arc<dyn LLMClient>,
    tx: UnboundedSender<Vec<Sensation<String>>>,
    batch_size: usize,
}

impl<M: MemoryStore + Send + Sync> RecallMotor<M> {
    /// Create a new motor backed by the given store and LLM.
    pub fn new(
        store: M,
        llm: Arc<dyn LLMClient>,
        tx: UnboundedSender<Vec<Sensation<String>>>,
        batch_size: usize,
    ) -> Self {
        Self {
            store,
            llm,
            tx,
            batch_size,
        }
    }

    async fn discover_and_summarize_recent(&self) -> Result<String, MotorError> {
        let recents = self
            .store
            .fetch_recent_impressions(self.batch_size)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        if recents.is_empty() {
            return Err(MotorError::Failed("no recent impressions".into()));
        }
        let moment_text = recents
            .iter()
            .map(|i| i.how.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let neighbors = self
            .store
            .retrieve_related_impressions(&moment_text, 3)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let ctx = json!({
            "moment": moment_text,
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
            trace!(token = %tok.text, "neighbor_llm_token");
            out.push_str(&tok.text);
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
            impression_ids: recents.iter().map(|m| m.id.clone()).collect(),
        };
        self.store
            .store_summary_impression(&stored, &stored.impression_ids)
            .await
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

    async fn discover_and_summarize_for(&self, sentence: &str) -> Result<String, MotorError> {
        let neighbors = self
            .store
            .retrieve_related_impressions(sentence, self.batch_size)
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        if neighbors.is_empty() {
            return Err(MotorError::Failed("no related impressions".into()));
        }
        let ctx = json!({
            "sentence": sentence,
            "neighbors": neighbors.iter().map(|n| n.how.clone()).collect::<Vec<_>>()
        });
        let prompt = format!(
            "These memories may be relevant to this sentence:\n{}\nSummarize them in one sentence.",
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
            trace!(token = %tok.text, "neighbor_llm_token");
            out.push_str(&tok.text);
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
            impression_ids: neighbors.iter().map(|m| m.id.clone()).collect(),
        };
        self.store
            .store_summary_impression(&stored, &stored.impression_ids)
            .await
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
impl<M: MemoryStore + Send + Sync> Motor for RecallMotor<M> {
    fn description(&self) -> &'static str {
        "Recall memories related to a sentence and summarize them.\n\
Example:\n\
<recall>I saw a red scarf on the bench.</recall>"
    }

    fn name(&self) -> &'static str {
        "recall"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "recall" {
            return Err(MotorError::Unrecognized);
        }
        let mut action = intention.action;
        let sentence = action.collect_text().await;
        if sentence.trim().is_empty() {
            return Err(MotorError::Failed("missing sentence".into()));
        }
        match self.discover_and_summarize_for(sentence.trim()).await {
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
impl<M: MemoryStore + Send + Sync> SensorDirectingMotor for RecallMotor<M> {
    fn attached_sensors(&self) -> Vec<String> {
        vec!["Recall".to_string()]
    }

    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError> {
        if sensor_name != "Recall" {
            return Err(MotorError::Failed(format!(
                "Unknown sensor: {}",
                sensor_name
            )));
        }
        let _ = self.discover_and_summarize_recent().await?;
        Ok(())
    }
}
