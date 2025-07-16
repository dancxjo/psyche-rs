use chrono::Utc;
use psyche::distiller::{Distiller, DistillerConfig};
use psyche::llm::mock_chat::MockChat;
use psyche::models::MemoryEntry;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn combobulator_config_distills_chat() {
    let cfg = DistillerConfig {
        name: "combobulator".into(),
        input_kind: "sensation/chat".into(),
        output_kind: "instant".into(),
        prompt_template: "{input}".into(),
        post_process: None,
    };
    let mut d = Distiller {
        config: cfg,
        llm: Box::new(MockChat::default()),
    };
    let entry = MemoryEntry {
        id: Uuid::new_v4(),
        kind: "sensation/chat".into(),
        when: Utc::now(),
        what: json!("I feel tired"),
        how: String::new(),
    };
    let out = d.distill(vec![entry]).await.unwrap();
    assert_eq!(out[0].kind, "instant");
    assert_eq!(out[0].how, "mock response");
}

#[tokio::test]
async fn memory_config_distills_instant() {
    let cfg = DistillerConfig {
        name: "memory".into(),
        input_kind: "instant".into(),
        output_kind: "situation".into(),
        prompt_template: "{input}".into(),
        post_process: None,
    };
    let mut d = Distiller {
        config: cfg,
        llm: Box::new(MockChat::default()),
    };
    let entry = MemoryEntry {
        id: Uuid::new_v4(),
        kind: "instant".into(),
        when: Utc::now(),
        what: json!("so sleepy"),
        how: String::from("so sleepy"),
    };
    let out = d.distill(vec![entry]).await.unwrap();
    assert_eq!(out[0].kind, "situation");
    assert_eq!(out[0].how, "mock response");
}
