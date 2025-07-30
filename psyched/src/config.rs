use indexmap::IndexMap;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct DistillerConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "input")]
    pub input: String,
    #[serde(default, rename = "output")]
    pub output: String,
    #[serde(default, rename = "prompt")]
    pub prompt: Option<String>,
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
    /// Path to a model file passed to the sensor when applicable.
    #[serde(default)]
    pub whisper_model: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipeConfig {
    pub socket: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

fn default_spoken_socket() -> String {
    "voice.sock".into()
}

fn default_tts_url() -> String {
    "http://localhost:5002".into()
}

fn default_speaker_id() -> String {
    "p330".into()
}

fn default_language_id() -> String {
    "".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpokenConfig {
    #[serde(default = "default_spoken_socket")]
    pub socket: String,
    #[serde(default = "default_tts_url")]
    pub tts_url: String,
    #[serde(default = "default_speaker_id")]
    pub speaker_id: String,
    #[serde(default = "default_language_id")]
    pub language_id: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Deserialize)]
pub struct PsycheConfig {
    #[serde(default)]
    pub wit: IndexMap<String, DistillerConfig>,
    #[serde(default)]
    pub sensor: IndexMap<String, SensorConfig>,
    #[serde(default)]
    pub pipe: IndexMap<String, PipeConfig>,
    #[serde(default)]
    pub spoken: Option<SpokenConfig>,
}

impl Default for PsycheConfig {
    fn default() -> Self {
        Self {
            wit: IndexMap::new(),
            sensor: IndexMap::new(),
            pipe: IndexMap::new(),
            spoken: None,
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
