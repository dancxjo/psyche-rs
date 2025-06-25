use llm::chat::{ChatMessage, ChatRole};

/// A single message in a conversation.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// The speaker of a [`Message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Pete's own utterances.
    Me,
    /// The interlocutor's statements.
    Interlocutor,
}

/// Conversation history with a system prompt context.
#[derive(Debug, Clone)]
pub struct Conversation {
    /// System prompt describing Pete's behaviour.
    pub system_prompt: String,
    /// Chronological messages exchanged.
    pub messages: Vec<Message>,
    /// Maximum number of tokens retained when building prompts.
    pub max_tokens: usize,
}

impl Conversation {
    /// Create a new conversation with the given `system_prompt` and token limit.
    pub fn new(system_prompt: String, max_tokens: usize) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            max_tokens,
        }
    }

    /// Record a heard utterance from `role`.
    pub fn hear(&mut self, role: Role, text: impl Into<String>) {
        self.messages.push(Message {
            role,
            content: text.into(),
        });
    }

    /// Return the most recent messages within `max_tokens` token budget.
    pub fn tail(&self) -> Vec<(ChatRole, String)> {
        let mut remaining = self.max_tokens;
        let mut collected: Vec<(ChatRole, String)> = Vec::new();
        for m in self.messages.iter().rev() {
            let tokens = m.content.split_whitespace().count();
            if tokens > remaining {
                break;
            }
            remaining -= tokens;
            let role = match m.role {
                Role::Me => ChatRole::Assistant,
                Role::Interlocutor => ChatRole::User,
            };
            collected.push((role, m.content.clone()));
        }
        collected.reverse();
        collected
    }

    /// Construct chat messages including the system prompt and pruned history.
    pub fn to_prompt(&self) -> Vec<ChatMessage> {
        let mut msgs = Vec::new();
        msgs.push(
            ChatMessage::user()
                .content(self.system_prompt.clone())
                .build(),
        );
        for (role, content) in self.tail() {
            let msg = match role {
                ChatRole::User => ChatMessage::user().content(content).build(),
                ChatRole::Assistant => ChatMessage::assistant().content(content).build(),
            };
            msgs.push(msg);
        }
        msgs
    }
}
