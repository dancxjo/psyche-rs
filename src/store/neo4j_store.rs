use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use neo4rs::{Graph, Node, query};

use crate::{Completion, Impression, Interruption, Memory, MemoryStore};

/// Memory store backed by a Neo4j graph database.
///
/// Each memory is stored as a node labelled with its type.
/// Properties are serialized into JSON strings for simplicity.
pub struct Neo4jStore {
    pub client: Graph,
}

#[async_trait::async_trait]
impl MemoryStore for Neo4jStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        let (label, mut props) = crate::codec::serialize_memory(memory)?;
        props.insert("memory_type".into(), serde_json::json!(label));
        let cypher = format!("MERGE (m:{label} {{ uuid: $uuid }}) SET m += $props");
        let json = serde_json::to_string(&props)?;
        self.client
            .run(
                query(&cypher)
                    .param("uuid", memory.uuid().to_string())
                    .param("props", json),
            )
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        let cypher = "MATCH (m {uuid: $uuid}) RETURN m";
        let mut stream = self
            .client
            .execute(query(cypher).param("uuid", uuid.to_string()))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        if let Some(row) = stream
            .next()
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?
        {
            let node: Node = row
                .get("m")
                .ok_or_else(|| anyhow::anyhow!("missing node"))?;
            let memory = crate::codec::deserialize_memory(&node)?;
            Ok(Some(memory))
        } else {
            Ok(None)
        }
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let cypher = "MATCH (m) RETURN m ORDER BY m.timestamp DESC LIMIT $l";
        let mut stream = self
            .client
            .execute(query(cypher).param("l", limit as i64))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        let mut out = Vec::new();
        while let Some(row) = stream
            .next()
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?
        {
            let node: Node = row
                .get("m")
                .ok_or_else(|| anyhow::anyhow!("missing node"))?;
            out.push(crate::codec::deserialize_memory(&node)?);
        }
        Ok(out)
    }

    async fn of_type(&self, type_name: &str, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let cypher = format!(
            "MATCH (m:{}) RETURN m ORDER BY m.timestamp DESC LIMIT $l",
            type_name
        );
        let mut stream = self
            .client
            .execute(query(&cypher).param("l", limit as i64))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        let mut out = Vec::new();
        while let Some(row) = stream
            .next()
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?
        {
            let node: Node = row
                .get("m")
                .ok_or_else(|| anyhow::anyhow!("missing node"))?;
            out.push(crate::codec::deserialize_memory(&node)?);
        }
        Ok(out)
    }

    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        let ts = since.duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let cypher = "MATCH (m) WHERE m.timestamp > $ts RETURN m ORDER BY m.timestamp ASC";
        let mut stream = self
            .client
            .execute(query(cypher).param("ts", ts))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        let mut out = Vec::new();
        while let Some(row) = stream
            .next()
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?
        {
            let node: Node = row
                .get("m")
                .ok_or_else(|| anyhow::anyhow!("missing node"))?;
            out.push(crate::codec::deserialize_memory(&node)?);
        }
        Ok(out)
    }

    async fn impressions_containing(&self, keyword: &str) -> anyhow::Result<Vec<Impression>> {
        let cypher = "MATCH (m:Impression) WHERE toLower(m.how) CONTAINS toLower($kw) RETURN m ORDER BY m.timestamp ASC";
        let mut stream = self
            .client
            .execute(query(cypher).param("kw", keyword))
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        let mut out = Vec::new();
        while let Some(row) = stream
            .next()
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?
        {
            let node: Node = row
                .get("m")
                .ok_or_else(|| anyhow::anyhow!("missing node"))?;
            if let Memory::Impression(i) = crate::codec::deserialize_memory(&node)? {
                out.push(i);
            }
        }
        Ok(out)
    }

    async fn complete_intention(
        &self,
        intention_id: Uuid,
        completion: Completion,
    ) -> anyhow::Result<()> {
        self.save(&Memory::Completion(completion.clone())).await?;
        let cypher = "MATCH (i:Intention {uuid: $iid}), (c:Completion {uuid: $cid}) CREATE (c)-[:COMPLETES]->(i)";
        self.client
            .run(
                query(cypher)
                    .param("iid", intention_id.to_string())
                    .param("cid", completion.uuid.to_string()),
            )
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        Ok(())
    }

    async fn interrupt_intention(
        &self,
        intention_id: Uuid,
        interruption: Interruption,
    ) -> anyhow::Result<()> {
        self.save(&Memory::Interruption(interruption.clone()))
            .await?;
        let cypher = "MATCH (i:Intention {uuid: $iid}), (r:Interruption {uuid: $rid}) CREATE (r)-[:INTERRUPTS]->(i)";
        self.client
            .run(
                query(cypher)
                    .param("iid", intention_id.to_string())
                    .param("rid", interruption.uuid.to_string()),
            )
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
        Ok(())
    }
}
