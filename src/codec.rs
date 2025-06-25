use neo4rs::Node;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::memory::*;

pub fn serialize_memory(memory: &Memory) -> (&'static str, HashMap<String, Value>) {
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
            ("Sensation", map)
        }
        Memory::Impression(i) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(i.uuid.to_string()));
            map.insert("how".into(), json!(i.how));
            map.insert("topic".into(), json!(i.topic));
            map.insert(
                "composed_of".into(),
                json!(
                    i.composed_of
                        .iter()
                        .map(|u| u.to_string())
                        .collect::<Vec<_>>()
                ),
            );
            map.insert(
                "timestamp".into(),
                json!(i.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            ("Impression", map)
        }
        Memory::Urge(u) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(u.uuid.to_string()));
            map.insert("source".into(), json!(u.source.to_string()));
            map.insert("motor_name".into(), json!(u.motor_name));
            map.insert("parameters".into(), u.parameters.clone());
            map.insert("intensity".into(), json!(u.intensity));
            map.insert(
                "timestamp".into(),
                json!(u.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs()),
            );
            ("Urge", map)
        }
        Memory::Intention(i) => {
            let mut map = HashMap::new();
            map.insert("uuid".into(), json!(i.uuid.to_string()));
            map.insert("urge".into(), json!(i.urge.to_string()));
            map.insert("motor_name".into(), json!(i.motor_name));
            map.insert("parameters".into(), i.parameters.clone());
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
            ("Intention", map)
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
            ("Completion", map)
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
            ("Interruption", map)
        }
        Memory::Of(_) => panic!("cannot serialize custom memory"),
    }
}

pub fn deserialize_memory(_node: &Node) -> anyhow::Result<Memory> {
    Err(anyhow::anyhow!("not implemented"))
}
