use neo4rs::{query, Graph};
use serde_json::to_string as json_to_string;
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
