use serde::Deserialize;

/// Configuration for a single Wit used by `psyched`.
fn default_priority() -> usize {
    0
}

fn default_beat_mod() -> usize {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct WitConfig {
    /// Memory kind this Wit consumes.
    pub input: String,
    /// Memory kind this Wit outputs.
    pub output: String,
    /// Prompt passed to the language model. Pipeline wits treat this as a
    /// template where `"{input}"` will be replaced with recent memories.
    pub prompt: String,
    /// Execution priority for conversational wits. Lower values run more often.
    #[serde(default = "default_priority")]
    pub priority: usize,
    /// Beat interval for pipeline wits. A value of `1` runs on every beat.
    #[serde(default = "default_beat_mod")]
    pub beat_mod: usize,
    /// Optional name of another Wit to receive this Wit’s output as input.
    #[serde(default)]
    pub feedback: Option<String>,
    /// Optional name of the LLM profile this Wit should use.
    #[serde(default)]
    pub llm: Option<String>,
    /// Optional postprocessing behavior.
    #[serde(default)]
    pub postprocess: Option<String>,
}
