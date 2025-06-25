use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensation {
    pub uuid: Uuid,
    pub kind: String,
    pub from: String,
    pub payload: serde_json::Value,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impression {
    pub uuid: Uuid,
    pub how: String,
    pub topic: String,
    pub composed_of: Vec<Uuid>,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Urge {
    pub uuid: Uuid,
    pub source: Uuid,
    pub motor_name: String,
    pub parameters: serde_json::Value,
    pub intensity: f32,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentionStatus {
    Pending,
    InProgress,
    Completed,
    Interrupted,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    pub uuid: Uuid,
    pub urge: Uuid,
    pub motor_name: String,
    pub parameters: serde_json::Value,
    pub issued_at: SystemTime,
    pub resolved_at: Option<SystemTime>,
    pub status: IntentionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotorCall {
    pub uuid: Uuid,
    pub xml: String,
    pub acknowledged_at: Option<SystemTime>,
    pub feedback_stream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub uuid: Uuid,
    pub intention: Uuid,
    pub outcome: String,
    pub transcript: Option<String>,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interruption {
    pub uuid: Uuid,
    pub intention: Uuid,
    pub reason: String,
    pub timestamp: SystemTime,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "memory_type", content = "data")]
pub enum Memory {
    Sensation(Sensation),
    Impression(Impression),
    Urge(Urge),
    Intention(Intention),
    Completion(Completion),
    Interruption(Interruption),

    #[serde(skip)]
    Of(Box<dyn Any + Send + Sync>),
}
impl Clone for Memory {
    fn clone(&self) -> Self {
        match self {
            Memory::Sensation(x) => Memory::Sensation(x.clone()),
            Memory::Impression(x) => Memory::Impression(x.clone()),
            Memory::Urge(x) => Memory::Urge(x.clone()),
            Memory::Intention(x) => Memory::Intention(x.clone()),
            Memory::Completion(x) => Memory::Completion(x.clone()),
            Memory::Interruption(x) => Memory::Interruption(x.clone()),
            Memory::Of(_) => panic!("cannot clone custom memory"),
        }
    }
}

impl Memory {
    pub fn try_as<T: 'static>(&self) -> Option<&T> {
        match self {
            Memory::Of(boxed) => boxed.downcast_ref::<T>(),
            _ => None,
        }
    }

    pub fn uuid(&self) -> Uuid {
        match self {
            Memory::Sensation(x) => x.uuid,
            Memory::Impression(x) => x.uuid,
            Memory::Urge(x) => x.uuid,
            Memory::Intention(x) => x.uuid,
            Memory::Completion(x) => x.uuid,
            Memory::Interruption(x) => x.uuid,
            Memory::Of(_) => Uuid::nil(),
        }
    }

    pub fn timestamp(&self) -> Option<SystemTime> {
        match self {
            Memory::Sensation(x) => Some(x.timestamp),
            Memory::Impression(x) => Some(x.timestamp),
            Memory::Urge(x) => Some(x.timestamp),
            Memory::Intention(x) => Some(x.issued_at),
            Memory::Completion(x) => Some(x.timestamp),
            Memory::Interruption(x) => Some(x.timestamp),
            Memory::Of(_) => None,
        }
    }
}

#[async_trait::async_trait]
pub trait MemoryStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()>;
    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<Memory>>;
    async fn of_type(&self, type_name: &str, limit: usize) -> anyhow::Result<Vec<Memory>>;
    async fn complete_intention(
        &self,
        intention_id: Uuid,
        completion: Completion,
    ) -> anyhow::Result<()>;
    async fn interrupt_intention(
        &self,
        intention_id: Uuid,
        interruption: Interruption,
    ) -> anyhow::Result<()>;
}
