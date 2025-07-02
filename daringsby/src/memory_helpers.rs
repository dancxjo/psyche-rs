use chrono::Utc;
use psyche_rs::{Impression, MemoryStore, StoredImpression, StoredSensation};
use tracing::{debug, error};

/// Persist an impression to the provided store.
///
/// Clones the sensation data to avoid ownership issues.
pub fn persist_impression<T: serde::Serialize>(
    store: &dyn MemoryStore,
    imp: &Impression<T>,
    kind: &str,
) -> anyhow::Result<()> {
    debug!("persisting impression");
    let mut sensation_ids = Vec::new();
    for s in &imp.what {
        let sid = uuid::Uuid::new_v4().to_string();
        sensation_ids.push(sid.clone());
        let stored = StoredSensation {
            id: sid,
            kind: s.kind.clone(),
            when: s.when.with_timezone(&Utc),
            data: serde_json::to_string(&s.what)?,
        };
        store.store_sensation(&stored).map_err(|e| {
            error!(?e, "store_sensation failed");
            e
        })?;
    }
    let stored_imp = StoredImpression {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        when: Utc::now(),
        how: imp.how.clone(),
        sensation_ids,
        impression_ids: Vec::new(),
    };
    store.store_impression(&stored_imp).map_err(|e| {
        error!(?e, "store_impression failed");
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use psyche_rs::{InMemoryStore, Sensation};

    #[test]
    fn stores_impression_and_sensations() {
        let store = InMemoryStore::new();
        let sensation = Sensation {
            kind: "test".into(),
            when: Local::now(),
            what: "hi".to_string(),
            source: None,
        };
        let imp = Impression {
            how: "example".into(),
            what: vec![sensation],
        };
        assert!(persist_impression(&store, &imp, "Instant").is_ok());
    }
}
