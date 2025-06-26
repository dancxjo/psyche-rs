#[test]
fn parse_action_stream() {
    let xml = r#"<say pitch="high">Hello world</say>"#;
    let parsed = psyche_rs::stream_parser::parse_streamed_action(xml).unwrap();

    assert_eq!(parsed.action.name, "say");
    assert_eq!(
        parsed.action.attributes["pitch"],
        serde_json::Value::String("high".into())
    );
    assert_eq!(parsed.body, "Hello world");
}
