use serde::Deserialize;

/// Configuration for a single Wit used by `psyched`.
#[derive(Debug, Clone, Deserialize)]
pub struct WitConfig {
    /// Memory kind this Wit consumes.
    pub input: String,
    /// Memory kind this Wit outputs.
    pub output: String,
    /// System prompt passed to the language model.
    pub prompt: String,
    /// Execution priority. Lower values run more often.
    pub priority: usize,
    /// Optional name of another Wit to receive this Wit’s output as input.
    #[serde(default)]
    pub feedback: Option<String>,
}
