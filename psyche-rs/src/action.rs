use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

use crate::memory::IntentionStatus;

/// Describes a potential act Pete might perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Action {
    /// Name of the action, typically matching a motor command.
    pub name: String,
    /// Arbitrary attributes parsed from motor XML or other sources.
    pub attributes: Value,
}

impl Action {
    /// Create a new [`Action`] with the provided name and attributes.
    pub fn new(name: impl Into<String>, attributes: impl Into<Value>) -> Self {
        Self {
            name: name.into(),
            attributes: attributes.into(),
        }
    }
}

/// Full record of an issued action from urge through completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enactment {
    pub uuid: Uuid,
    /// UUID of the originating [`Impression`].
    pub source: Uuid,
    pub action: Action,
    pub issued_at: SystemTime,
    pub resolved_at: Option<SystemTime>,
    pub status: IntentionStatus,
}

/// Convenience builders for [`Action`].
pub mod action {
    use super::Action;
    use serde_json::json;

    /// Create an [`Action`] with no attributes.
    pub fn of(name: &str) -> Action {
        Action::new(name, json!({}))
    }

    /// Create an [`Action`] using the supplied attributes.
    pub fn with(name: &str, attributes: impl Into<serde_json::Value>) -> Action {
        Action::new(name, attributes)
    }
}

/// Convenience builders for [`Urge`].
pub mod urge {
    use super::Action;
    use crate::memory::Urge;
    use std::time::SystemTime;
    use uuid::Uuid;

    /// Construct a new [`Urge`] targeting the given action.
    pub fn to(action: Action, source: Uuid) -> Urge {
        Urge {
            uuid: Uuid::new_v4(),
            source,
            action,
            intensity: 1.0,
            timestamp: SystemTime::now(),
        }
    }
}
