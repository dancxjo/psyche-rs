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
    /// IDs of impressions summarized by this one. Empty for regular impressions.
    #[serde(rename = "summary_of", default)]
    pub impression_ids: Vec<String>,
}

/// Trait for interacting with Pete's memory storage.
pub trait MemoryStore {
    /// Insert a new sensation. Implementations should avoid duplicating
    /// previously stored sensations.
    fn store_sensation(&self, sensation: &StoredSensation) -> anyhow::Result<()>;

    /// Insert a new impression. Implementations are responsible for
    /// persisting the impression and storing its embedding in Qdrant.
    fn store_impression(&self, impression: &StoredImpression) -> anyhow::Result<()>;

    /// Insert a summary impression linked to other impressions.
    fn store_summary_impression(
        &self,
        summary: &StoredImpression,
        linked_ids: &[String],
    ) -> anyhow::Result<()> {
        let _ = linked_ids;
        self.store_impression(summary)
    }

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

    /// Fetch the most recent impressions from storage.
    fn fetch_recent_impressions(&self, limit: usize) -> anyhow::Result<Vec<StoredImpression>>;

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

#[cfg(test)]
impl InMemoryStore {
    pub fn sensation_count(&self) -> usize {
        self.sensations.lock().unwrap().len()
    }

    pub fn impression_count(&self) -> usize {
        self.impressions.lock().unwrap().len()
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

    fn fetch_recent_impressions(&self, limit: usize) -> anyhow::Result<Vec<StoredImpression>> {
        let mut imps: Vec<_> = self.impressions.lock().unwrap().values().cloned().collect();
        imps.sort_by_key(|i| std::cmp::Reverse(i.when));
        imps.truncate(limit);
        Ok(imps)
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

impl<M> MemoryStore for std::sync::Arc<M>
where
    M: MemoryStore + ?Sized,
{
    fn store_sensation(&self, s: &StoredSensation) -> anyhow::Result<()> {
        (**self).store_sensation(s)
    }

    fn store_impression(&self, i: &StoredImpression) -> anyhow::Result<()> {
        (**self).store_impression(i)
    }

    fn store_summary_impression(
        &self,
        summary: &StoredImpression,
        linked_ids: &[String],
    ) -> anyhow::Result<()> {
        (**self).store_summary_impression(summary, linked_ids)
    }

    fn add_lifecycle_stage(
        &self,
        impression_id: &str,
        stage: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        (**self).add_lifecycle_stage(impression_id, stage, detail)
    }

    fn retrieve_related_impressions(
        &self,
        query_how: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<StoredImpression>> {
        (**self).retrieve_related_impressions(query_how, top_k)
    }

    fn fetch_recent_impressions(&self, limit: usize) -> anyhow::Result<Vec<StoredImpression>> {
        (**self).fetch_recent_impressions(limit)
    }

    fn load_full_impression(
        &self,
        impression_id: &str,
    ) -> anyhow::Result<(
        StoredImpression,
        Vec<StoredSensation>,
        HashMap<String, String>,
    )> {
        (**self).load_full_impression(impression_id)
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
            impression_ids: Vec::new(),
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

#[cfg(test)]
mod integration_tests {
    use super::*;
    use chrono::Utc;

    fn make_sensation(id: &str) -> StoredSensation {
        StoredSensation {
            id: id.into(),
            kind: "visual.face".into(),
            when: Utc::now(),
            data: r#"{"face_id":"abc123"}"#.into(),
        }
    }

    fn make_impression(id: &str, sensation_ids: Vec<String>) -> StoredImpression {
        StoredImpression {
            id: id.into(),
            kind: "Situation".into(),
            when: Utc::now(),
            how: "Saw a familiar face.".into(),
            sensation_ids,
            impression_ids: Vec::new(),
        }
    }

    #[test]
    fn store_sensation_once_and_link_to_multiple_impressions() {
        let store = InMemoryStore::new();
        let sensation = make_sensation("s1");
        store.store_sensation(&sensation).unwrap();

        let imp1 = make_impression("i1", vec![sensation.id.clone()]);
        let imp2 = make_impression("i2", vec![sensation.id.clone()]);
        store.store_impression(&imp1).unwrap();
        store.store_impression(&imp2).unwrap();

        let (imp1_loaded, sens1, _) = store.load_full_impression("i1").unwrap();
        let (imp2_loaded, sens2, _) = store.load_full_impression("i2").unwrap();

        assert_eq!(imp1_loaded.id, "i1");
        assert_eq!(imp2_loaded.id, "i2");
        assert_eq!(sens1, vec![sensation.clone()]);
        assert_eq!(sens2, vec![sensation.clone()]);
    }

    #[test]
    fn lifecycle_stage_linking_and_retrieval() {
        let store = InMemoryStore::new();
        let sensation = make_sensation("s2");
        store.store_sensation(&sensation).unwrap();
        let imp = make_impression("i3", vec![sensation.id.clone()]);
        store.store_impression(&imp).unwrap();

        store
            .add_lifecycle_stage("i3", "Intention", "wanted to greet")
            .unwrap();
        store
            .add_lifecycle_stage("i3", "Action", "waved hand")
            .unwrap();

        let (_, _, stages) = store.load_full_impression("i3").unwrap();
        assert_eq!(stages.get("Intention"), Some(&"wanted to greet".into()));
        assert_eq!(stages.get("Action"), Some(&"waved hand".into()));
    }

    #[test]
    fn related_impressions_via_retrieve_related_impressions_mock() {
        let store = InMemoryStore::new();
        for i in 0..10 {
            let s = make_sensation(&format!("s{}", i));
            store.store_sensation(&s).unwrap();
            let imp = make_impression(&format!("i{}", i), vec![s.id.clone()]);
            store.store_impression(&imp).unwrap();
        }

        let related = store
            .retrieve_related_impressions("Saw a familiar face.", 3)
            .unwrap();
        assert_eq!(related.len(), 3);
    }

    #[test]
    fn consolidates_duplicate_sensations() {
        let store = InMemoryStore::new();
        let mut s = make_sensation("dup");
        store.store_sensation(&s).unwrap();
        s.data = "changed".into();
        store.store_sensation(&s).unwrap();

        let imp = make_impression("i_dup", vec!["dup".into()]);
        store.store_impression(&imp).unwrap();
        let (_, sens, _) = store.load_full_impression("i_dup").unwrap();
        assert_eq!(sens.len(), 1);
        assert_eq!(sens[0].data, r#"{"face_id":"abc123"}"#);
    }

    #[test]
    fn fetch_recent_returns_latest() {
        let store = InMemoryStore::new();
        for i in 0..3 {
            let s = make_sensation(&format!("s{}", i));
            store.store_sensation(&s).unwrap();
            let mut imp = make_impression(&format!("i{}", i), vec![s.id.clone()]);
            imp.when = imp.when + chrono::Duration::seconds(i as i64);
            store.store_impression(&imp).unwrap();
        }
        let latest = store.fetch_recent_impressions(2).unwrap();
        assert_eq!(latest.len(), 2);
        assert!(latest[0].when >= latest[1].when);
    }
}
