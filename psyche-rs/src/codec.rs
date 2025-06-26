use neo4rs::Node;
use serde_json::to_string as json_to_string;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::UNIX_EPOCH;
use uuid::Uuid;

use crate::memory::*;

/// Serialize a [`Memory`] value into a Neo4j node label and property map.
///
/// The property map uses JSON values so it can easily be converted to the
/// [`neo4rs`] value types when constructing Cypher queries.
pub fn serialize_memory(memory: &Memory) -> anyhow::Result<(&'static str, HashMap<String, Value>)> {
    match memory {
        Memory::Sensation(s) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(s.uuid.to_string()));
            map.insert("kind".into(), json!(s.kind));
            map.insert("from".into(), json!(s.from));
            map.insert("payload".into(), s.payload.clone());
            map.insert(
                "timestamp".into(),
                json!(s.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            Ok(("Sensation", map))
        }
        Memory::Impression(i) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(i.uuid.to_string()));
            map.insert("how".into(), json!(i.how));
            map.insert("topic".into(), json!(i.topic));
            let comp: Vec<String> = i.composed_of.iter().map(|u| u.to_string()).collect();
            map.insert("composed_of".into(), json!(json_to_string(&comp)?));
            map.insert(
                "timestamp".into(),
                json!(i.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            Ok(("Impression", map))
        }
        Memory::Urge(u) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(u.uuid.to_string()));
            map.insert("source".into(), json!(u.source.to_string()));
            map.insert("motor_name".into(), json!(u.action.name));
            map.insert("parameters".into(), u.action.attributes.clone());
            map.insert("intensity".into(), json!(u.intensity));
            map.insert(
                "timestamp".into(),
                json!(u.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            Ok(("Urge", map))
        }
        Memory::Intention(i) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(i.uuid.to_string()));
            map.insert("urge".into(), json!(i.urge.to_string()));
            map.insert("motor_name".into(), json!(i.action.name));
            map.insert("parameters".into(), i.action.attributes.clone());
            map.insert(
                "issued_at".into(),
                json!(i.issued_at.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            if let Some(ts) = i.resolved_at {
                map.insert(
                    "resolved_at".into(),
                    json!(ts.duration_since(UNIX_EPOCH).unwrap().as_secs()),
                );
            }
            map.insert("status".into(), json!(format!("{:?}", i.status)));
            Ok(("Intention", map))
        }
        Memory::Completion(c) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(c.uuid.to_string()));
            map.insert("intention".into(), json!(c.intention.to_string()));
            map.insert("outcome".into(), json!(c.outcome));
            if let Some(t) = &c.transcript {
                map.insert("transcript".into(), json!(t));
            }
            map.insert(
                "timestamp".into(),
                json!(c.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            Ok(("Completion", map))
        }
        Memory::Interruption(r) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(r.uuid.to_string()));
            map.insert("intention".into(), json!(r.intention.to_string()));
            map.insert("reason".into(), json!(r.reason));
            map.insert(
                "timestamp".into(),
                json!(r.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            Ok(("Interruption", map))
        }
        Memory::Of(_) => Err(anyhow::anyhow!("cannot serialize custom memory")),
    }
}

/// Convert a Neo4j [`Node`] back into a [`Memory`] instance.
pub fn deserialize_memory(node: &Node) -> anyhow::Result<Memory> {
    let labels = node.labels();
    let label = labels.get(0).map(|s| s.as_str()).unwrap_or("");
    match label {
        "Sensation" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let kind: String = node
                .get("kind")
                .ok_or_else(|| anyhow::anyhow!("missing kind"))?;
            let from: String = node
                .get("from")
                .ok_or_else(|| anyhow::anyhow!("missing from"))?;
            let payload_str: String = node
                .get("payload")
                .ok_or_else(|| anyhow::anyhow!("missing payload"))?;
            let ts: i64 = node
                .get("timestamp")
                .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
            let payload = serde_json::from_str(&payload_str)?;
            let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
            Ok(Memory::Sensation(Sensation {
                uuid: Uuid::parse_str(&uuid)?,
                kind,
                from,
                payload,
                timestamp,
            }))
        }
        "Impression" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let how: String = node
                .get("how")
                .ok_or_else(|| anyhow::anyhow!("missing how"))?;
            let topic: String = node
                .get("topic")
                .ok_or_else(|| anyhow::anyhow!("missing topic"))?;
            let list_str: String = node
                .get("composed_of")
                .ok_or_else(|| anyhow::anyhow!("missing composed_of"))?;
            let ts: i64 = node
                .get("timestamp")
                .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
            let names: Vec<String> = serde_json::from_str(&list_str)?;
            let uuids: Vec<Uuid> = names.iter().map(|s| Uuid::parse_str(s).unwrap()).collect();
            let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
            Ok(Memory::Impression(Impression {
                uuid: Uuid::parse_str(&uuid)?,
                how,
                topic,
                composed_of: uuids,
                timestamp,
            }))
        }
        "Urge" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let source: String = node
                .get("source")
                .ok_or_else(|| anyhow::anyhow!("missing source"))?;
            let motor_name: String = node
                .get("motor_name")
                .ok_or_else(|| anyhow::anyhow!("missing motor_name"))?;
            let parameters_str: String = node
                .get("parameters")
                .ok_or_else(|| anyhow::anyhow!("missing parameters"))?;
            let intensity: f64 = node
                .get("intensity")
                .ok_or_else(|| anyhow::anyhow!("missing intensity"))?;
            let ts: i64 = node
                .get("timestamp")
                .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
            let parameters: serde_json::Value = serde_json::from_str(&parameters_str)?;
            let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
            Ok(Memory::Urge(Urge {
                uuid: Uuid::parse_str(&uuid)?,
                source: Uuid::parse_str(&source)?,
                action: crate::action::Action::new(motor_name, parameters),
                intensity: intensity as f32,
                timestamp,
            }))
        }
        "Intention" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let urge: String = node
                .get("urge")
                .ok_or_else(|| anyhow::anyhow!("missing urge"))?;
            let motor_name: String = node
                .get("motor_name")
                .ok_or_else(|| anyhow::anyhow!("missing motor_name"))?;
            let parameters_str: String = node
                .get("parameters")
                .ok_or_else(|| anyhow::anyhow!("missing parameters"))?;
            let issued_at: i64 = node
                .get("issued_at")
                .ok_or_else(|| anyhow::anyhow!("missing issued_at"))?;
            let status: String = node
                .get("status")
                .ok_or_else(|| anyhow::anyhow!("missing status"))?;
            let resolved_at: Option<i64> = node.get("resolved_at");
            let parameters: serde_json::Value = serde_json::from_str(&parameters_str)?;
            let issued_at = UNIX_EPOCH + std::time::Duration::from_secs(issued_at as u64);
            let resolved_at_time =
                resolved_at.map(|t| UNIX_EPOCH + std::time::Duration::from_secs(t as u64));
            let status = match status.as_str() {
                "Pending" => IntentionStatus::Pending,
                "InProgress" => IntentionStatus::InProgress,
                "Completed" => IntentionStatus::Completed,
                "Interrupted" => IntentionStatus::Interrupted,
                other => IntentionStatus::Failed(other.to_string()),
            };
            Ok(Memory::Intention(Intention {
                uuid: Uuid::parse_str(&uuid)?,
                urge: Uuid::parse_str(&urge)?,
                action: crate::action::Action::new(motor_name, parameters),
                issued_at,
                resolved_at: resolved_at_time,
                status,
            }))
        }
        "Completion" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let intention: String = node
                .get("intention")
                .ok_or_else(|| anyhow::anyhow!("missing intention"))?;
            let outcome: String = node
                .get("outcome")
                .ok_or_else(|| anyhow::anyhow!("missing outcome"))?;
            let transcript: Option<String> = node.get("transcript");
            let ts: i64 = node
                .get("timestamp")
                .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
            let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
            Ok(Memory::Completion(Completion {
                uuid: Uuid::parse_str(&uuid)?,
                intention: Uuid::parse_str(&intention)?,
                outcome,
                transcript,
                timestamp,
            }))
        }
        "Interruption" => {
            let uuid: String = node
                .get("uuid")
                .ok_or_else(|| anyhow::anyhow!("missing uuid"))?;
            let intention: String = node
                .get("intention")
                .ok_or_else(|| anyhow::anyhow!("missing intention"))?;
            let reason: String = node
                .get("reason")
                .ok_or_else(|| anyhow::anyhow!("missing reason"))?;
            let ts: i64 = node
                .get("timestamp")
                .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
            let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
            Ok(Memory::Interruption(Interruption {
                uuid: Uuid::parse_str(&uuid)?,
                intention: Uuid::parse_str(&intention)?,
                reason,
                timestamp,
            }))
        }
        _ => Ok(Memory::Of(Box::new(format!("{:?}", node)))),
    }
}
