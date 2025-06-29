use crate::memory_store::{MemoryStore, StoredImpression, StoredSensation};
use std::collections::HashMap;

/// `MemorySensor` queries memory for impressions related to the current
/// situation and returns them for further processing. Logging is provided for
/// debugging.
pub struct MemorySensor<M: MemoryStore> {
    pub store: M,
    /// Number of related impressions to request from the store.
    pub top_k: usize,
}

impl<M: MemoryStore> MemorySensor<M> {
    /// Create a new sensor that retrieves up to `top_k` related impressions.
    pub fn new(store: M, top_k: usize) -> Self {
        Self { store, top_k }
    }

    /// Query memory for related impressions and return full details. This can
    /// then be emitted as sensations or impressions in the wider system.
    pub async fn sense_related_memory(
        &self,
        current_how: &str,
    ) -> anyhow::Result<
        Vec<(
            StoredImpression,
            Vec<StoredSensation>,
            HashMap<String, String>,
        )>,
    > {
        let related = self
            .store
            .retrieve_related_impressions(current_how, self.top_k)?;
        let mut out = Vec::new();
        for imp in related {
            let data = self.store.load_full_impression(&imp.id)?;
            tracing::debug!(?data, "related impression");
            out.push(data);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_store::{InMemoryStore, StoredImpression, StoredSensation};
    use chrono::Utc;

    #[tokio::test]
    async fn sense_logs_related_impressions() {
        let store = InMemoryStore::new();
        let sensation = StoredSensation {
            id: "s1".into(),
            kind: "test".into(),
            when: Utc::now(),
            data: "{}".into(),
        };
        store.store_sensation(&sensation).unwrap();
        let impression = StoredImpression {
            id: "i1".into(),
            kind: "Situation".into(),
            when: Utc::now(),
            how: "example".into(),
            sensation_ids: vec!["s1".into()],
        };
        store.store_impression(&impression).unwrap();

        let sensor = MemorySensor::new(store, 1);
        let res = sensor.sense_related_memory("example").await.unwrap();
        assert_eq!(res.len(), 1);
    }
}
