use psyche::llm::prompt::PromptHelper;
use psyche::llm::{
    mock_chat::MockChat, mock_embed::MockEmbed, LlmCapability, LlmProfile, LlmRegistry,
};
use psyche::memory::{InMemoryBackend, Memorizer, MemoryBackend};
use tracing_test::traced_test;

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
        prompter: PromptHelper::default(),
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
        prompter: PromptHelper::default(),
    };
    let stored = memorizer
        .memorize("full text", None, true, vec![])
        .await
        .unwrap();
    assert_eq!(stored.experience.how, "mock response");
}

#[tokio::test]
async fn provided_summary_truncated() {
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
        prompter: PromptHelper::default(),
    };
    let stored = memorizer
        .memorize("full text", Some("Short. Extra."), false, vec![])
        .await
        .unwrap();
    assert_eq!(stored.experience.how, "Short.");
}

use async_trait::async_trait;
use std::sync::Mutex;
use tokio_stream::{iter, Stream};

#[derive(Default)]
struct SpyChat {
    system: Mutex<Option<String>>,
    user: Mutex<Option<String>>,
}

#[async_trait(?Send)]
impl psyche::llm::CanChat for SpyChat {
    async fn chat_stream(
        &self,
        _profile: &psyche::llm::LlmProfile,
        system: &str,
        user: &str,
    ) -> anyhow::Result<Box<dyn Stream<Item = String> + Unpin>> {
        *self.system.lock().unwrap() = Some(system.to_string());
        *self.user.lock().unwrap() = Some(user.to_string());
        Ok(Box::new(iter(["ok".to_string()])))
    }
}

#[tokio::test]
async fn prefixes_prompt_with_self_header() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("identity.toml"), "[dummy]\n").unwrap();
    std::fs::write(dir.path().join("self.txt"), "You are Layka").unwrap();
    let prompter = PromptHelper::from_config(&dir.path().join("identity.toml"));

    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Chat],
    };
    let chat = SpyChat::default();
    let backend = InMemoryBackend::default();
    let memorizer = Memorizer {
        chat: Some(&chat),
        embed: &MockEmbed::default(),
        profile: &profile,
        backend: &backend,
        prompter,
    };

    memorizer
        .memorize("body", None, true, vec![])
        .await
        .unwrap();

    assert_eq!(
        chat.system.lock().unwrap().as_deref(),
        Some("You are Layka")
    );
}

#[tokio::test]
async fn search_returns_neighbors() {
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
        prompter: PromptHelper::default(),
    };

    let stored = memorizer
        .memorize("hello world", None, true, vec![])
        .await
        .unwrap();
    let neighbors = backend.search(&stored.vector, 1).await.unwrap();
    assert_eq!(neighbors[0], stored.experience);
}

#[traced_test]
#[tokio::test]
async fn memorize_emits_logs() {
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
        prompter: PromptHelper::default(),
    };
    memorizer
        .memorize("body", None, true, vec!["test".into()])
        .await
        .unwrap();
    assert!(logs_contain("memorize called"));
    assert!(logs_contain("storing experience"));
}
