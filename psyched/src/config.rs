use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct DistillerConfig {
    pub name: String,
    #[serde(rename = "input")] // for config readability
    pub input_kind: String,
    #[serde(rename = "output")]
    pub output_kind: String,
    #[serde(default)]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PsycheConfig {
    #[serde(default)]
    pub distillers: Vec<DistillerConfig>,
}

impl Default for PsycheConfig {
    fn default() -> Self {
        Self {
            distillers: Vec::new(),
        }
    }
}

/// Load a [`PsycheConfig`] from a TOML file.
///
/// # Examples
///
/// ```no_run
/// use psyched::config::load;
/// # tokio_test::block_on(async {
/// let cfg = load("tests/configs/psyche.toml").await.unwrap();
/// assert!(!cfg.distillers.is_empty());
/// # });
/// ```
pub async fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<PsycheConfig> {
    let text = tokio::fs::read_to_string(path).await?;
    Ok(toml::from_str(&text)?)
}
