use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents an action request with streaming body content.
pub struct Action {
    /// Name of the action to perform.
    pub name: String,
    /// Structured parameters for the action.
    pub params: Value,
    /// Live body stream associated with the action.
    pub body: BoxStream<'static, String>,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("name", &self.name)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl Action {
    /// Helper to create an [`Action`] from parts.
    pub fn new(name: impl Into<String>, params: Value, body: BoxStream<'static, String>) -> Self {
        Self {
            name: name.into(),
            params,
            body,
        }
    }
}

/// A motor capable of performing an action.
pub trait Motor {
    /// Returns a brief description of the motor's purpose.
    fn description(&self) -> &'static str;
    /// Attempt to perform the provided action.
    ///
    /// The implementor may consume the action body stream as desired.
    fn perform(&self, action: Action) -> Result<(), MotorError>;
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
    /// Parameters for the action.
    pub params: Value,
}

impl Urge {
    /// Convenience constructor.
    pub fn to(name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
        }
    }
}

/// Metadata stating the intent to perform an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    /// Action name.
    pub name: String,
    /// Parameters for the action.
    pub params: Value,
}

impl Intention {
    /// Convenience constructor.
    pub fn to(name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
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
    fn action_new_sets_fields() {
        let body = stream::empty().boxed();
        let mut action = Action::new("test", Value::Null, body);
        assert_eq!(action.name, "test");
        let none = futures::executor::block_on(async { action.body.next().await });
        assert!(none.is_none());
    }
}
