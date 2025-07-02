use ollama_rs::generation::chat::ChatMessage;

/// Tracks messages exchanged with the LLM.
///
/// [`Conversation`] retains the full history while exposing
/// a sliding window via [`tail`].
#[derive(Debug, Default)]
pub struct Conversation {
    history: Vec<ChatMessage>,
    max_tail_len: usize,
}

impl Conversation {
    /// Create a new conversation keeping `max_tail_len` messages in the tail.
    pub fn new(max_tail_len: usize) -> Self {
        Self {
            history: Vec::new(),
            max_tail_len,
        }
    }

    /// Append a user message.
    pub fn push_user(&mut self, content: impl Into<String>) {
        self.history.push(ChatMessage::user(content.into()));
    }

    /// Append an assistant message.
    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.history.push(ChatMessage::assistant(content.into()));
    }

    /// Append a system message.
    pub fn push_system(&mut self, content: impl Into<String>) {
        self.history.push(ChatMessage::system(content.into()));
    }

    /// Return the most recent messages up to the configured limit.
    pub fn tail(&self) -> Vec<ChatMessage> {
        let start = self.history.len().saturating_sub(self.max_tail_len);
        self.history[start..].to_vec()
    }

    /// Full conversation history.
    pub fn full(&self) -> &[ChatMessage] {
        &self.history
    }
}
