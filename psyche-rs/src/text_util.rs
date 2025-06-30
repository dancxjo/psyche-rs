use crate::Impression;
use serde::Serialize;
use serde_json::{self, Value};

/// Converts a serializable value to plain text.
///
/// Strings are returned without surrounding quotes. [`Impression`] instances are
/// rendered as quoted summary sentences. All other values are serialized to JSON
/// using [`serde_json`].
///
/// # Examples
///
/// ```
/// use psyche_rs::{Impression, text_util::to_plain_text};
///
/// let imp = Impression::<()>::new(Vec::new(), "He waved.").unwrap();
/// assert_eq!(to_plain_text(&imp), "\"He waved.\"");
/// assert_eq!(to_plain_text(&"hello"), "hello");
/// ```
#[inline]
pub fn to_plain_text<T: Serialize>(value: &T) -> String {
    let val = match serde_json::to_value(value) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    if let Ok(imp) = serde_json::from_value::<Impression<Value>>(val.clone()) {
        return format!("\"{}\"", imp.how);
    }

    match val {
        Value::String(s) => s,
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Impression;

    #[test]
    fn string_is_unquoted() {
        assert_eq!(to_plain_text(&"hello"), "hello");
    }

    #[test]
    fn numbers_become_json() {
        assert_eq!(to_plain_text(&42), "42");
    }

    #[test]
    fn impression_is_quoted_summary() {
        let imp = Impression::new(Vec::<crate::Sensation<String>>::new(), "Hi.").unwrap();
        assert_eq!(to_plain_text(&imp), "\"Hi.\"");
    }
}
