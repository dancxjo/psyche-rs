use serde_json::Value;

/// Return the first sentence of `text` including the ending punctuation.
///
/// Splits on '.', '!' or '?' and returns the substring up to and including the
/// first such character. If none are found, the entire string is returned.
///
/// # Examples
///
/// ```
/// use psyche::utils::first_sentence;
/// assert_eq!(first_sentence("Hello world. Second."), "Hello world.");
/// assert_eq!(first_sentence("No punctuation"), "No punctuation");
/// ```
pub fn first_sentence(text: &str) -> String {
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            return text[..=i].trim().to_string();
        }
    }
    text.trim().to_string()
}

/// Attempt to parse `text` as JSON, falling back to a string value.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use psyche::utils::parse_json_or_string;
/// assert_eq!(parse_json_or_string("{\"a\":1}"), json!({"a":1}));
/// assert_eq!(parse_json_or_string("text"), json!("text"));
/// ```
pub fn parse_json_or_string(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}
