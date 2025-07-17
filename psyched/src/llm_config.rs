use serde::Deserialize;
use std::path::Path;

/// Configuration for an individual LLM provider.
#[derive(Debug, Deserialize, Clone)]
pub struct LlmProviderConfig {
    /// Provider name like `"ollama"` or `"openai"`.
    pub provider: String,
    /// Base URL for the service if applicable (e.g. Ollama).
    #[serde(default)]
    pub base_url: Option<String>,
    /// API key for services like OpenAI.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Models offered by this provider.
    pub models: Vec<String>,
    /// Supported capabilities such as `"chat"` or `"embedding"`.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LlmConfigFile {
    #[serde(rename = "llm")]
    pub llms: Vec<LlmProviderConfig>,
}

/// Loads LLM configuration from `path`, returning a registry and profile
/// using the first configured provider and its first model.
pub async fn load_first_llm(
    path: &Path,
) -> anyhow::Result<(psyche::llm::LlmRegistry, psyche::llm::LlmProfile)> {
    use tokio::fs;

    let text = fs::read_to_string(path).await?;
    let cfg: LlmConfigFile = toml::from_str(&text)?;
    let first = cfg
        .llms
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no llm entries"))?;
    let model = first
        .models
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no models for provider"))?;
    let capabilities = if first.capabilities.is_empty() {
        vec![psyche::llm::LlmCapability::Chat]
    } else {
        first
            .capabilities
            .iter()
            .filter_map(|c| match c.as_str() {
                "chat" => Some(psyche::llm::LlmCapability::Chat),
                "embedding" => Some(psyche::llm::LlmCapability::Embedding),
                _ => None,
            })
            .collect()
    };
    let profile = psyche::llm::LlmProfile {
        provider: first.provider.clone(),
        model: model.clone(),
        capabilities,
    };
    let registry = match first.provider.as_str() {
        "ollama" => psyche::llm::LlmRegistry {
            chat: Box::new(psyche::llm::ollama::OllamaChat {
                base_url: first
                    .base_url
                    .unwrap_or_else(|| "http://localhost:11434".into()),
                model,
            }),
            embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
        },
        "mock" => psyche::llm::LlmRegistry {
            chat: Box::new(psyche::llm::mock_chat::MockChat::default()),
            embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
        },
        other => anyhow::bail!("unsupported provider: {}", other),
    };
    Ok((registry, profile))
}
