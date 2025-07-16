use psyche::llm::{
    mock_chat::MockChat, mock_embed::MockEmbed, LlmCapability, LlmProfile, LlmRegistry,
};
use psyche::memory::{InMemoryBackend, Memorizer};

#[tokio::test]
async fn uses_provided_summary() {
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Chat, LlmCapability::Embedding],
    };
    let registry = LlmRegistry {
        chat: Box::new(MockChat::default()),
        embed: Box::new(MockEmbed::default()),
    };
    let backend = InMemoryBackend::default();
    let memorizer = Memorizer {
        chat: Some(&*registry.chat),
        embed: &*registry.embed,
        profile: &profile,
        backend: &backend,
    };
    let stored = memorizer
        .memorize("full text", Some("short"), false, vec![])
        .await
        .unwrap();
    assert_eq!(stored.experience.how, "short");
    assert_eq!(stored.vector, vec![0.0, 0.0, 0.0]);
}

#[tokio::test]
async fn generates_summary_with_llm() {
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Chat, LlmCapability::Embedding],
    };
    let registry = LlmRegistry {
        chat: Box::new(MockChat::default()),
        embed: Box::new(MockEmbed::default()),
    };
    let backend = InMemoryBackend::default();
    let memorizer = Memorizer {
        chat: Some(&*registry.chat),
        embed: &*registry.embed,
        profile: &profile,
        backend: &backend,
    };
    let stored = memorizer
        .memorize("full text", None, true, vec![])
        .await
        .unwrap();
    assert_eq!(stored.experience.how, "mock response");
}
