use chrono::Utc;
use psyche::llm::mock_chat::MockChat;
use psyche::llm::{LlmCapability, LlmProfile};
use psyche::models::MemoryEntry;
use psyche::wit::{link_sources, Wit, WitConfig};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn combobulator_config_distills_chat() {
    let entry_id = Uuid::new_v4();
    let cfg = WitConfig {
        name: "combobulator".into(),
        input_kind: "sensation/chat".into(),
        output_kind: "instant".into(),
        prompt_template: "{input}".into(),
        post_process: Some(link_sources),
    };
    let mut d = Wit {
        config: cfg,
        llm: Box::new(MockChat::default()),
        profile: LlmProfile {
            provider: "mock".into(),
            model: "mock".into(),
            capabilities: vec![LlmCapability::Chat],
        },
    };
    let entry = MemoryEntry {
        id: entry_id,
        kind: "sensation/chat".into(),
        when: Utc::now(),
        what: json!("I feel tired"),
        how: String::new(),
    };
    let out = d.distill(vec![entry]).await.unwrap();
    assert_eq!(out[0].kind, "instant");
    assert_eq!(out[0].how, "mock response");
    assert_eq!(out[0].what, json!([entry_id]));
}

#[tokio::test]
async fn prefix_filter_matches_subkind() {
    let entry_id = Uuid::new_v4();
    let cfg = WitConfig {
        name: "combobulator".into(),
        input_kind: "sensation".into(),
        output_kind: "instant".into(),
        prompt_template: "{input}".into(),
        post_process: Some(link_sources),
    };
    let mut d = Wit {
        config: cfg,
        llm: Box::new(MockChat::default()),
        profile: LlmProfile {
            provider: "mock".into(),
            model: "mock".into(),
            capabilities: vec![LlmCapability::Chat],
        },
    };
    let entry = MemoryEntry {
        id: entry_id,
        kind: "sensation/chat".into(),
        when: Utc::now(),
        what: json!("I feel tired"),
        how: String::new(),
    };
    let out = d.distill(vec![entry]).await.unwrap();
    assert_eq!(out[0].kind, "instant");
    assert_eq!(out[0].what, json!([entry_id]));
}

#[tokio::test]
async fn memory_config_distills_instant() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let cfg = WitConfig {
        name: "memory".into(),
        input_kind: "instant".into(),
        output_kind: "situation".into(),
        prompt_template: "{input}".into(),
        post_process: Some(link_sources),
    };
    let mut d = Wit {
        config: cfg,
        llm: Box::new(MockChat::default()),
        profile: LlmProfile {
            provider: "mock".into(),
            model: "mock".into(),
            capabilities: vec![LlmCapability::Chat],
        },
    };
    let entry1 = MemoryEntry {
        id: id1,
        kind: "instant".into(),
        when: Utc::now(),
        what: json!("so sleepy"),
        how: String::from("so sleepy"),
    };
    let entry2 = MemoryEntry {
        id: id2,
        kind: "instant".into(),
        when: Utc::now(),
        what: json!("very tired"),
        how: String::from("very tired"),
    };
    let out = d.distill(vec![entry1, entry2]).await.unwrap();
    assert_eq!(out[0].kind, "situation");
    assert_eq!(out[0].how, "mock response");
    assert_eq!(out[0].what, json!([id1, id2]));
}
