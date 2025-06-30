use serde::Serialize;
use serde_json::Value;

/// Converts a serializable value to plain text.
///
/// Strings are returned without surrounding quotes. Other values are serialized
/// to JSON using [`serde_json`].
#[inline]
pub fn to_plain_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(v) => v.to_string(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_is_unquoted() {
        assert_eq!(to_plain_text(&"hello"), "hello");
    }

    #[test]
    fn numbers_become_json() {
        assert_eq!(to_plain_text(&42), "42");
    }
}
