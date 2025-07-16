use psyche::llm::{
    mock_chat::MockChat, mock_embed::MockEmbed, LlmCapability, LlmProfile, LlmRegistry,
};
use tokio_stream::StreamExt;

#[tokio::test]
async fn mock_chat_streams() {
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Chat],
    };
    let registry = LlmRegistry {
        chat: Box::new(MockChat::default()),
        embed: Box::new(MockEmbed::default()),
    };
    let stream = registry
        .chat
        .chat_stream(&profile, "", "hello")
        .await
        .expect("stream");
    let collected: Vec<String> = stream.collect().await;
    assert_eq!(collected, vec!["mock response".to_string()]);
}

#[tokio::test]
async fn mock_embed_returns_vector() {
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Embedding],
    };
    let registry = LlmRegistry {
        chat: Box::new(MockChat::default()),
        embed: Box::new(MockEmbed::default()),
    };
    let vec = registry.embed.embed(&profile, "test").await.expect("embed");
    assert_eq!(vec, vec![0.0, 0.0, 0.0]);
}
