use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::SystemTime;
use uuid::Uuid;

use crate::action::Action;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensation {
    pub uuid: Uuid,
    pub kind: String,
    pub from: String,
    pub payload: serde_json::Value,
    pub timestamp: SystemTime,
}

impl Sensation {
    /// Construct a simple text sensation originating from `from`.
    ///
    /// ```
    /// use psyche_rs::Sensation;
    /// let s = Sensation::new_text("hi", "tester");
    /// assert_eq!(s.kind, "text/plain");
    /// ```
    pub fn new_text(content: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            kind: "text/plain".into(),
            from: from.into(),
            payload: serde_json::json!({ "content": content.into() }),
            timestamp: SystemTime::now(),
        }
    }
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
    /// The action Pete feels compelled to perform.
    pub action: Action,
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
    /// The action Pete intends to enact.
    pub action: Action,
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

/// A simple emotion record capturing how Pete feels about a specific event.
///
/// The [`subject`] field links to the UUID of the originating memory (for
/// example a [`Completion`] or [`Interruption`]).
///
/// ```
/// use psyche_rs::{Emotion, Completion};
/// use std::time::SystemTime;
/// use uuid::Uuid;
///
/// let completion = Completion {
///     uuid: Uuid::new_v4(),
///     intention: Uuid::new_v4(),
///     outcome: "done".into(),
///     transcript: None,
///     timestamp: SystemTime::now(),
/// };
///
/// let emotion = Emotion {
///     uuid: Uuid::new_v4(),
///     subject: completion.uuid,
///     mood: "proud".into(),
///     reason: "I feel proud for completing the task.".into(),
///     timestamp: SystemTime::now(),
/// };
/// assert_eq!(emotion.subject, completion.uuid);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emotion {
    pub uuid: Uuid,
    pub subject: Uuid,
    pub mood: String,
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
    /// Attempt to borrow a custom memory stored via [`Memory::Of`].
    ///
    /// # Examples
    ///
    /// ```
    /// use psyche_rs::Memory;
    /// let m = Memory::Of(Box::new(42u32));
    /// assert_eq!(m.try_as::<u32>(), Some(&42u32));
    /// assert!(m.try_as::<i32>().is_none());
    /// ```
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
            Memory::Of(boxed) => {
                if let Some(emo) = boxed.downcast_ref::<Emotion>() {
                    emo.uuid
                } else {
                    Uuid::nil()
                }
            }
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
            Memory::Of(boxed) => {
                if let Some(emo) = boxed.downcast_ref::<Emotion>() {
                    Some(emo.timestamp)
                } else {
                    None
                }
            }
        }
    }
}

#[async_trait::async_trait]
pub trait MemoryStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()>;
    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<Memory>>;
    async fn of_type(&self, type_name: &str, limit: usize) -> anyhow::Result<Vec<Memory>>;
    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>>;
    async fn impressions_containing(&self, keyword: &str) -> anyhow::Result<Vec<Impression>>;
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
