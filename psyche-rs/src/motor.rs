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

/// Motors that can direct sensors implement this trait in addition to [`Motor`].
///
/// Implementors are able to activate one or more sensors and should return the
/// set of known sensor identifiers from [`attached_sensors`]. The
/// [`direct_sensor`] method requests activation of a specific sensor and returns
/// an error if the name is not recognized.
#[async_trait::async_trait]
pub trait SensorDirectingMotor: Motor {
    /// Names or identifiers of sensors this motor is able to control.
    fn attached_sensors(&self) -> Vec<String>;

    /// Instruct the motor to activate the given sensor.
    async fn direct_sensor(&self, sensor_name: &str) -> Result<(), MotorError>;
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
    /// Creates a new `Intention` from an action name and parameters.
    ///
    /// Note: The `assigned_motor` field is left empty and must be set by the
    /// caller using [`Intention::assign`].
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
    /// Optional completion metadata.
    pub completion: Option<Completion>,
    /// Optional interruption metadata.
    pub interruption: Option<Interruption>,
}

impl<T> Default for ActionResult<T> {
    fn default() -> Self {
        Self {
            sensations: Vec::new(),
            completed: false,
            completion: None,
            interruption: None,
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
        assert_eq!(action.intention.urge.name, "test");
        let none = futures::executor::block_on(async { action.body.next().await });
        assert!(none.is_none());
    }

    #[test]
    fn streaming_body_collects_values() {
        let body = stream::iter(vec!["one".to_string(), "two".to_string()]).boxed();
        let mut action = Action::new("test", Value::Null, body);
        let collected = futures::executor::block_on(async {
            let mut chunks = Vec::new();
            while let Some(chunk) = action.body.next().await {
                chunks.push(chunk);
            }
            chunks
        });
        assert_eq!(collected, vec!["one", "two"]);
    }

    #[test]
    fn action_result_with_completion_and_interruption() {
        let completion = Completion::of("look", Value::Null);
        let interruption = Interruption::of("look", Value::Null);
        let result = ActionResult::<Value> {
            sensations: Vec::new(),
            completed: false,
            completion: Some(completion.clone()),
            interruption: Some(interruption.clone()),
        };
        assert_eq!(result.completion.unwrap().name, "look");
        assert_eq!(result.interruption.unwrap().name, "look");
    }
}
