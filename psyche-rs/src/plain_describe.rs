use serde::Serialize;

use crate::{Sensation, text_util::to_plain_text};

/// Trait for converting types to a plain text representation.
///
/// This is used when building prompts for language models so that
/// structured data can be embedded as readable text.
///
/// # Examples
///
/// ```no_run
/// use chrono::Local;
/// use psyche_rs::{Sensation, PlainDescribe};
///
/// let s = Sensation::<String> {
///     kind: "utterance.text".into(),
///     when: Local::now(),
///     what: "hello".into(),
///     source: None,
/// };
/// assert_eq!(s.to_plain(), "hello");
/// ```
pub trait PlainDescribe {
    /// Convert the type to plain text.
    fn to_plain(&self) -> String;
}

impl<T> PlainDescribe for Sensation<T>
where
    T: Serialize,
{
    fn to_plain(&self) -> String {
        to_plain_text(&self.what)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    #[test]
    fn converts_sensation_payload() {
        let s = Sensation {
            kind: "utterance".into(),
            when: Local::now(),
            what: "hi".to_string(),
            source: None,
        };
        assert_eq!(s.to_plain(), "hi");
    }
}
