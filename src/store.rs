use neo4rs::{Graph, query};
use serde_json::to_string as json_to_string;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{Completion, Interruption, Memory, MemoryStore};

pub struct Neo4jMemoryStore {
    pub graph: Arc<Graph>,
}

#[async_trait::async_trait]
impl MemoryStore for Neo4jMemoryStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        let (label, props) = crate::codec::serialize_memory(memory)?;
        let json = json_to_string(&props)?;
        let cypher = format!("CREATE (m:{} $props)", label);
        // store props as json string until proper Bolt conversion is implemented
        self.graph
            .run(query(&cypher).param("props", json))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        Ok(())
    }

    async fn get_by_uuid(&self, _uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(None)
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn of_type(&self, _type_name: &str, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn recent_since(&self, _since: std::time::SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn impressions_containing(
        &self,
        _keyword: &str,
    ) -> anyhow::Result<Vec<crate::Impression>> {
        Ok(Vec::new())
    }

    async fn complete_intention(
        &self,
        _intention_id: Uuid,
        _completion: Completion,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(
        &self,
        _intention_id: Uuid,
        _interruption: Interruption,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Simple in-memory store useful for examples and tests.
pub struct DummyStore {
    data: Arc<tokio::sync::Mutex<HashMap<Uuid, Memory>>>,
}

impl DummyStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            data: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for DummyStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        let stored = match memory {
            Memory::Of(boxed) => {
                if let Some(e) = boxed.downcast_ref::<crate::Emotion>() {
                    Memory::Of(Box::new(e.clone()))
                } else {
                    Memory::Of(Box::new(()))
                }
            }
            other => other.clone(),
        };
        self.data.lock().await.insert(memory.uuid(), stored);
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        let guard = self.data.lock().await;
        Ok(guard.get(&uuid).cloned())
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn of_type(&self, _type_name: &str, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn recent_since(&self, _since: std::time::SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }

    async fn impressions_containing(
        &self,
        _keyword: &str,
    ) -> anyhow::Result<Vec<crate::Impression>> {
        Ok(vec![])
    }

    async fn complete_intention(
        &self,
        _intention_id: Uuid,
        _completion: Completion,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(
        &self,
        _intention_id: Uuid,
        _interruption: Interruption,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
