use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::sensation::Sensation;

/// Represents an action request with streaming body content.
pub struct Action {
    /// Desired intention to be fulfilled by the motor.
    pub intention: Intention,
    /// Live body stream associated with the action.
    pub body: BoxStream<'static, String>,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("intention", &self.intention)
            .finish_non_exhaustive()
    }
}

impl Action {
    /// Helper to create an [`Action`] from parts.
    pub fn new(name: impl Into<String>, args: Value, body: BoxStream<'static, String>) -> Self {
        Self {
            intention: Intention::to(name, args),
            body,
        }
    }
}

/// A motor capable of performing an action.
#[async_trait::async_trait]
pub trait Motor: Send + Sync {
    /// Returns a brief description of the motor's purpose.
    fn description(&self) -> &'static str;
    /// Attempt to perform the provided action.
    async fn perform(&self, action: Action) -> Result<ActionResult, MotorError>;
    /// Name of the motor action handled.
    fn name(&self) -> &'static str;
}

/// Errors that may occur while executing a motor action.
#[derive(Debug, thiserror::Error)]
pub enum MotorError {
    /// The motor did not recognize the requested action.
    #[error("Unrecognized action")]
    Unrecognized,

    /// The motor failed to complete the action for the given reason.
    #[error("Execution failed: {0}")]
    Failed(String),
}

/// Metadata describing an intended action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Urge {
    /// Action name.
    pub name: String,
    /// Named arguments for the action.
    pub args: HashMap<String, String>,
    /// Optional body text included with the urge.
    pub body: Option<String>,
}

impl Urge {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: HashMap::new(),
            body: None,
        }
    }
}

/// Metadata stating the intent to perform an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    /// Original urge that led to this intention.
    pub urge: Urge,
    /// Name of the motor assigned to satisfy the urge.
    pub assigned_motor: String,
}

impl Intention {
    /// Convenience constructor.
    pub fn to(name: impl Into<String>, params: Value) -> Self {
        let mut urge = Urge::new(name);
        // back-compat with legacy params when converting from actions
        for (k, v) in params.as_object().cloned().unwrap_or_default() {
            let val = if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            };
            urge.args.insert(k, val);
        }
        Self {
            urge,
            assigned_motor: String::new(),
        }
    }

    /// Create a new intention from an urge and motor name.
    pub fn assign(urge: Urge, motor: impl Into<String>) -> Self {
        Self {
            urge,
            assigned_motor: motor.into(),
        }
    }
}

/// Metadata describing an interruption to an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interruption {
    /// Action name.
    pub name: String,
    /// Parameters for the action.
    pub params: Value,
    /// Optional reason for interruption.
    pub reason: Option<String>,
}

impl Interruption {
    /// Convenience constructor.
    pub fn of(name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
            reason: None,
        }
    }
}

/// Metadata describing the completion of an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    /// Action name.
    pub name: String,
    /// Parameters for the action.
    pub params: Value,
    /// Optional result description.
    pub result: Option<String>,
}

impl Completion {
    /// Convenience constructor.
    pub fn of(name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
            result: None,
        }
    }
}

/// Outcome from executing a motor action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult<T = serde_json::Value> {
    /// Sensations produced by the motor.
    pub sensations: Vec<Sensation<T>>,
    /// Whether the action fully completed.
    pub completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};

    #[test]
    fn action_new_sets_fields() {
        let body = stream::empty().boxed();
        let mut action = Action::new("test", Value::Null, body);
        assert_eq!(action.intention.urge.name, "test");
        let none = futures::executor::block_on(async { action.body.next().await });
        assert!(none.is_none());
    }
}
