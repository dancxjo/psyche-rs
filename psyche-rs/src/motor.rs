use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A command issued by the "Will" for a motor to carry out.
///
/// The command payload `T` defaults to [`serde_json::Value`].
///
/// # Examples
///
/// ```
/// use psyche_rs::MotorCommand;
///
/// let cmd: MotorCommand = MotorCommand {
///     name: "say".into(),
///     args: serde_json::json!({"volume": 11}),
///     content: Some("Hello".into()),
/// };
/// assert_eq!(cmd.name, "say");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotorCommand<T = serde_json::Value> {
    /// Target motor name.
    pub name: String,
    /// Structured arguments for the motor.
    pub args: T,
    /// Optional free-form text.
    pub content: Option<String>,
}

/// A device capable of executing motor commands.
#[async_trait(?Send)]
pub trait Motor<T = serde_json::Value> {
    /// Carry out the provided command.
    async fn execute(&mut self, command: MotorCommand<T>);
}

/// Dispatches commands to registered motors.
#[async_trait(?Send)]
pub trait MotorExecutor<T = serde_json::Value> {
    /// Route the command to an appropriate motor.
    async fn submit(&mut self, command: MotorCommand<T>);
}
