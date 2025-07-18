use serde::Deserialize;
use std::path::Path;

/// Configuration for an individual LLM provider.
#[derive(Debug, Deserialize, Clone)]
pub struct LlmProviderConfig {
    /// Provider name like `"ollama"` or `"openai"`.
    pub provider: String,
    /// Optional unique name used for selecting this provider.
    #[serde(default)]
    pub name: Option<String>,
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
    /// Maximum concurrent requests allowed.
    #[serde(default)]
    pub concurrency: Option<usize>,
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

/// Loads all LLM providers from `path` returning initialized instances.
pub async fn load_llms(path: &Path) -> anyhow::Result<Vec<psyche::llm::LlmInstance>> {
    use tokio::fs;
    let text = fs::read_to_string(path).await?;
    let cfg: LlmConfigFile = toml::from_str(&text)?;
    let mut out = Vec::new();
    for (idx, prov) in cfg.llms.into_iter().enumerate() {
        let model = prov
            .models
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no models for provider"))?;
        let capabilities = if prov.capabilities.is_empty() {
            vec![psyche::llm::LlmCapability::Chat]
        } else {
            prov.capabilities
                .iter()
                .filter_map(|c| match c.as_str() {
                    "chat" => Some(psyche::llm::LlmCapability::Chat),
                    "embedding" => Some(psyche::llm::LlmCapability::Embedding),
                    _ => None,
                })
                .collect()
        };
        let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
            provider: prov.provider.clone(),
            model: model.clone(),
            capabilities,
        });
        let chat: std::sync::Arc<dyn psyche::llm::CanChat> = match prov.provider.as_str() {
            "ollama" => std::sync::Arc::new(psyche::llm::ollama::OllamaChat {
                base_url: prov
                    .base_url
                    .unwrap_or_else(|| "http://localhost:11434".into()),
                model,
            }),
            "mock" => std::sync::Arc::new(psyche::llm::mock_chat::MockChat::default()),
            other => anyhow::bail!("unsupported provider: {}", other),
        };
        let name = prov
            .name
            .clone()
            .unwrap_or_else(|| format!("{}{}", prov.provider, idx));
        let concurrency = prov.concurrency.unwrap_or(1);
        out.push(psyche::llm::LlmInstance {
            name,
            chat,
            profile,
            semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency)),
        });
    }
    Ok(out)
}
