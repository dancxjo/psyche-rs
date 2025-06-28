use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Represents an action request with streaming body content.
pub struct Action {
    /// Metadata describing what is intended.
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
    /// Helper to create an [`Action`] from an [`Intention`].
    pub fn from_intention(intention: Intention, body: BoxStream<'static, String>) -> Self {
        Self { intention, body }
    }
}

/// A motor capable of performing an action.
#[async_trait::async_trait]
pub trait Motor {
    /// Name of the motor command this handler performs.
    fn name(&self) -> &'static str;
    /// Returns a brief description of the motor's purpose.
    fn description(&self) -> &'static str;
    /// Attempt to perform the provided action.
    ///
    /// The implementor may consume the action body stream as desired.
    async fn perform(&self, action: Action) -> Result<ActionResult, MotorError>;
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

/// Result of performing an action.
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// Sensations produced by the motor.
    pub sensations: Vec<crate::Sensation>,
    /// Whether the action completed successfully.
    pub completed: bool,
}

/// Metadata describing an intended action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Urge {
    /// Action name, e.g. `"look"`.
    pub name: String,
    /// Structured arguments for the urge.
    pub args: HashMap<String, String>,
    /// Optional body content for streaming motors.
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
    /// Originating urge.
    pub urge: Urge,
    /// Motor selected to fulfill the urge.
    pub assigned_motor: String,
}

impl Intention {
    /// Creates a new intention for the given motor.
    pub fn new(urge: Urge, motor: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};

    #[test]
    fn action_from_intention_sets_fields() {
        let body = stream::empty().boxed();
        let urge = Urge::new("test");
        let intention = Intention::new(urge, "m");
        let mut action = Action::from_intention(intention, body);
        assert_eq!(action.intention.urge.name, "test");
        let none = futures::executor::block_on(async { action.body.next().await });
        assert!(none.is_none());
    }
}
