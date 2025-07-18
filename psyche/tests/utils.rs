use serde_json::json;

#[test]
fn first_sentence_extracts_sentence() {
    assert_eq!(psyche::utils::first_sentence("Hello. World."), "Hello.");
}

#[test]
fn parse_json_or_string_parses_json() {
    assert_eq!(
        psyche::utils::parse_json_or_string("{\"a\":1}"),
        json!({"a":1})
    );
    assert_eq!(psyche::utils::parse_json_or_string("hi"), json!("hi"));
}
