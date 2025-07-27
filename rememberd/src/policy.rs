use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Deserialize)]
struct RawPolicy {
    #[serde(default)]
    recall: RecallSection,
}

#[derive(Deserialize, Default)]
struct RecallSection {
    #[serde(default)]
    kinds: Vec<String>,
}

/// Runtime policy controlling automatic behavior.
#[derive(Clone, Default)]
pub struct Policy {
    recall_kinds: HashSet<String>,
}

impl Policy {
    /// Load policy from `policy.toml` if present.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("policy.toml");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(raw) = toml::from_str::<RawPolicy>(&text) {
                return Self {
                    recall_kinds: raw.recall.kinds.into_iter().collect(),
                };
            }
        }
        Self::default()
    }

    /// Whether a new entry of `kind` should trigger recall.
    pub fn recall_for(&self, kind: &str) -> bool {
        self.recall_kinds.contains(kind)
    }
}
