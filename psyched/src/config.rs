use indexmap::IndexMap;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct DistillerConfig {
    #[serde(default)]
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

fn default_sensor_enabled() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SensorConfig {
    #[serde(default = "default_sensor_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub socket: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Deserialize)]
pub struct PsycheConfig {
    #[serde(default)]
    pub wit: IndexMap<String, DistillerConfig>,
    #[serde(default)]
    pub sensor: IndexMap<String, SensorConfig>,
}

impl Default for PsycheConfig {
    fn default() -> Self {
        Self {
            wit: IndexMap::new(),
            sensor: IndexMap::new(),
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
/// assert!(!cfg.wit.is_empty());
/// # });
/// ```
pub async fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<PsycheConfig> {
    let text = tokio::fs::read_to_string(path).await?;
    Ok(toml::from_str(&text)?)
}
