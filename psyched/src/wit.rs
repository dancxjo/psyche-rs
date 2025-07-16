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
    /// Number of beats between each execution.
    pub every: usize,
}
