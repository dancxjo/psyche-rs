use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A generic Sensation type for the Pete runtime.
///
/// The payload type `T` defaults to [`serde_json::Value`].
///
/// # Examples
///
/// Creating a typed sensation:
///
/// ```
/// use chrono::Local;
/// use psyche_rs::Sensation;
///
/// let s: Sensation<String> = Sensation {
///     kind: "utterance.text".into(),
///     when: Local::now(),
///     what: "hello".into(),
///     source: Some("interlocutor".into()),
/// };
/// assert_eq!(s.what, "hello");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensation<T = serde_json::Value> {
    /// Category of sensation, e.g. `"utterance.text"`.
    pub kind: String,
    /// Timestamp for when the sensation occurred in **local time**.
    ///
    /// Local timestamps align better with LLM prompts so they
    /// reflect the agent's perceived clock rather than UTC.
    pub when: DateTime<Local>,
    /// Payload describing what was sensed.
    pub what: T,
    /// Optional origin identifier.
    pub source: Option<String>,
}
