use std::path::Path;

/// Helper that reads a self description from `self.txt` next to a config file.
/// When present, this text is used as the system prompt for LLM calls.
#[derive(Default, Clone)]
pub struct PromptHelper {
    header: Option<String>,
}

impl PromptHelper {
    /// Loads a [`PromptHelper`] using `config_path` to locate `self.txt`.
    ///
    /// ```
    /// use psyche::llm::prompt::PromptHelper;
    /// use std::path::Path;
    /// let helper = PromptHelper::from_config(Path::new("soul/identity.toml"));
    /// assert_eq!(helper.system(), "");
    /// ```
    pub fn from_config(config_path: &Path) -> Self {
        let dir = config_path.parent().unwrap_or_else(|| Path::new("."));
        let path = dir.join("self.txt");
        let header = std::fs::read_to_string(path).ok();
        Self { header }
    }

    /// Returns the loaded header, or an empty string if none was found.
    pub fn system(&self) -> &str {
        self.header.as_deref().unwrap_or("")
    }
}
