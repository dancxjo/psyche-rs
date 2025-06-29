use futures::stream::BoxStream;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use serde_json::Value;

use crate::sensation::Sensation;

/// Represents a motor command with streaming body content.
pub struct Action {
    /// Action name such as `say` or `look`.
    pub name: String,
    /// Parsed parameters from the action tag.
    pub params: Value,
    /// Live body stream associated with the action.
    pub body: BoxStream<'static, String>,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Action {
    /// Helper to create an [`Action`] from parts.
    ///
    /// ```
    /// use futures::stream::{self, StreamExt};
    /// use psyche_rs::Action;
    ///
    /// let body = stream::empty().boxed();
    /// let action = Action::new("log", serde_json::Value::Null, body);
    /// assert_eq!(action.name, "log");
    /// ```
    pub fn new(name: impl Into<String>, args: Value, body: BoxStream<'static, String>) -> Self {
        Self {
            name: name.into(),
            params: args,
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
    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError>;
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

/// Metadata stating the decision to perform a motor action.
#[derive(Debug)]
pub struct Intention {
    /// Action details to be executed by a motor.
    pub action: Action,
    /// Name of the motor assigned to handle the action.
    pub assigned_motor: String,
}

impl Intention {
    /// Creates a new [`Intention`] wrapping the provided [`Action`].
    ///
    /// The returned intention is not yet assigned to a motor. Use
    /// [`Intention::assign`] to direct it.
    pub fn to(action: Action) -> Self {
        Self {
            action,
            assigned_motor: String::new(),
        }
    }

    /// Assign the intention to a specific motor.
    pub fn assign(mut self, motor: impl Into<String>) -> Self {
        self.assigned_motor = motor.into();
        self
    }
}

impl Serialize for Intention {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("Intention", 3)?;
        s.serialize_field("name", &self.action.name)?;
        s.serialize_field("params", &self.action.params)?;
        s.serialize_field("assigned_motor", &self.assigned_motor)?;
        s.end()
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

    /// Build a [`Completion`] from an [`Action`]. Consumes the action and
    /// drops its body stream.
    ///
    /// ```
    /// use psyche_rs::{Action, Completion};
    /// use futures::stream::{self, StreamExt};
    /// let body = stream::empty().boxed();
    /// let action = Action::new("say", serde_json::Value::Null, body);
    /// let c = Completion::of_action(action);
    /// assert_eq!(c.name, "say");
    /// ```
    pub fn of_action(action: Action) -> Self {
        Self {
            name: action.name,
            params: action.params,
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
        assert_eq!(action.name, "test");
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
