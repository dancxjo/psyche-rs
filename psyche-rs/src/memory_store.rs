use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a sensation stored in memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredSensation {
    pub id: String,
    pub kind: String,
    pub when: DateTime<Utc>,
    pub data: String,
}

/// Represents an impression stored in memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredImpression {
    pub id: String,
    pub kind: String,
    pub when: DateTime<Utc>,
    pub how: String,
    pub sensation_ids: Vec<String>,
}

/// Trait for interacting with Pete's memory storage.
pub trait MemoryStore {
    /// Insert a new sensation. Implementations should avoid duplicating
    /// previously stored sensations.
    fn store_sensation(&self, sensation: &StoredSensation) -> anyhow::Result<()>;

    /// Insert a new impression. Implementations are responsible for
    /// persisting the impression and storing its embedding in Qdrant.
    fn store_impression(&self, impression: &StoredImpression) -> anyhow::Result<()>;

    /// Link an impression to a lifecycle stage. `detail` may contain a brief
    /// description of the stage.
    fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()>;

    /// Retrieve impressions related to a query string using vector search.
    fn retrieve_related_impressions(
        &self,
        query_how: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>>;

    /// Load the full impression, including associated sensations and lifecycle
    /// information. The returned `HashMap` maps stage names to their detail
    /// strings.
    fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        HashMap<String, String>,
    )>;
}

/// Simple in-memory implementation used for tests. This does **not** provide
/// persistence or vector search but mimics the API.
#[derive(Default)]
pub struct InMemoryStore {
    sensations: std::sync::Mutex<HashMap<String, StoredSensation>>,
    impressions: std::sync::Mutex<HashMap<String, StoredImpression>>,
    lifecycle: std::sync::Mutex<HashMap<String, HashMap<String, String>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryStore for InMemoryStore {
    fn store_sensation(&self, sensation: &StoredSensation) -> anyhow::Result<()> {
        self.sensations
            .lock()
            .unwrap()
            .entry(sensation.id.clone())
            .or_insert_with(|| sensation.clone());
        Ok(())
    }

    fn store_impression(&self, impression: &StoredImpression) -> anyhow::Result<()> {
        self.impressions
            .lock()
            .unwrap()
            .insert(impression.id.clone(), impression.clone());
        Ok(())
    }

    fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        let mut lc = self.lifecycle.lock().unwrap();
        let stages = lc.entry(impression_id.to_string()).or_default();
        stages.insert(stage.to_string(), detail.to_string());
        Ok(())
    }

    fn retrieve_related_impressions(
        &self,
        _query_how: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        let imps = self.impressions.lock().unwrap();
        Ok(imps.values().take(top_k).cloned().collect())
    }

    fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        HashMap<String, String>,
    )> {
        let imps = self.impressions.lock().unwrap();
        let imp = imps
            .get(impression_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("impression not found"))?;
        let sens_map = self.sensations.lock().unwrap();
        let sens = imp
            .sensation_ids
            .iter()
            .filter_map(|id| sens_map.get(id).cloned())
            .collect();
        let stages = self
            .lifecycle
            .lock()
            .unwrap()
            .get(impression_id)
            .cloned()
            .unwrap_or_default();
        Ok((imp, sens, stages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_sensation(id: &str) -> StoredSensation {
        StoredSensation {
            id: id.into(),
            kind: "test".into(),
            when: Utc::now(),
            data: "{}".into(),
        }
    }

    fn sample_impression(id: &str, sens: &[StoredSensation]) -> StoredImpression {
        StoredImpression {
            id: id.into(),
            kind: "Instant".into(),
            when: Utc::now(),
            how: "example".into(),
            sensation_ids: sens.iter().map(|s| s.id.clone()).collect(),
        }
    }

    #[test]
    fn store_and_load_round_trip() {
        let store = InMemoryStore::new();
        let s1 = sample_sensation("s1");
        store.store_sensation(&s1).unwrap();
        let imp = sample_impression("i1", &[s1.clone()]);
        store.store_impression(&imp).unwrap();
        store
            .add_lifecycle_stage(&imp.id, "Intention", "test")
            .unwrap();

        let (loaded_imp, sensations, stages) = store.load_full_impression(&imp.id).unwrap();
        assert_eq!(loaded_imp, imp);
        assert_eq!(sensations, vec![s1]);
        assert_eq!(stages.get("Intention"), Some(&"test".to_string()));
    }
}
