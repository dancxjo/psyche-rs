use serde::{Deserialize, Serialize};

use crate::sensation::Sensation;

/// A high-level summary of one or more sensations.
///
/// `Impression` bundles related sensations in [`what`] and expresses
/// them in natural language via [`how`].
///
/// # Examples
///
/// ```
/// use chrono::Local;
/// use psyche_rs::{Impression, Sensation};
///
/// let what = vec![Sensation::<String> {
///     kind: "utterance.text".into(),
///     when: Local::now(),
///     what: "salutations".into(),
///     source: None,
/// }];
/// let impression = Impression {
///     what: what.clone(),
///     how: "He said salutations".into(),
/// };
/// assert_eq!(impression.what[0].kind, "utterance.text");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impression<T = serde_json::Value> {
    /// Sensations that led to this impression.
    pub what: Vec<Sensation<T>>,
    /// Natural language summarizing the sensations.
    pub how: String,
}
