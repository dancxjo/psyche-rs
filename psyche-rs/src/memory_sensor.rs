use crate::memory_store::MemoryStore;

/// `MemorySensor` queries memory for impressions related to the current
/// situation and logs them. It can be plugged into a sensor pipeline to enrich
/// the agent's perception.
pub struct MemorySensor<M: MemoryStore> {
    pub store: M,
}

impl<M: MemoryStore> MemorySensor<M> {
    /// Query memory for related impressions and log them for now. In a full
    /// system this would emit sensations downstream.
    pub async fn sense_related_memory(&self, current_how: &str) -> anyhow::Result<()> {
        let related = self.store.retrieve_related_impressions(current_how, 5)?;
        for imp in related {
            let (imp_full, sensations, stages) = self.store.load_full_impression(&imp.id)?;
            tracing::debug!(?imp_full, ?sensations, ?stages, "related impression");
        }
        Ok(())
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

        let sensor = MemorySensor { store };
        sensor.sense_related_memory("example").await.unwrap();
    }
}
