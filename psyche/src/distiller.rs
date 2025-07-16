use crate::models::{Instant, Sensation};

/// Distills a raw `Sensation` into an `Instant` when possible.
///
/// Currently supports chat sensations of the form "I feel <emotion>".
///
/// # Examples
///
/// ```
/// use psyche::distiller::distill;
/// use psyche::models::Sensation;
///
/// let s = Sensation {
///     id: "1".into(),
///     path: "/chat".into(),
///     text: "I feel lonely".into(),
/// };
/// let instant = distill(&s).unwrap();
/// assert_eq!(instant.how, "The interlocutor feels lonely");
/// ```
pub fn distill(sensation: &Sensation) -> Option<Instant> {
    if sensation.path == "/chat" && sensation.text.starts_with("I feel ") {
        let feeling = sensation.text.trim_start_matches("I feel ");
        Some(Instant {
            kind: "instant".to_string(),
            how: format!("The interlocutor feels {}", feeling),
            what: vec![sensation.id.clone()],
        })
    } else {
        None
    }
}
