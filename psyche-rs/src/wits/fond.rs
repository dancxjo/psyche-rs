use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use uuid::Uuid;

use crate::llm::LLMExt;
use crate::memory::{Emotion, Memory, MemoryStore};
use crate::wit::Wit;
use llm::chat::ChatProvider;
use tracing::info;

/// `FondDuCoeur` observes completed or interrupted intentions and records
/// Pete's emotional response to them.
pub struct FondDuCoeur {
    events: VecDeque<Memory>,
    pub store: Arc<dyn MemoryStore>,
    pub llm: Arc<dyn ChatProvider>,
}

impl FondDuCoeur {
    /// Create a new [`FondDuCoeur`] wit.
    pub fn new(store: Arc<dyn MemoryStore>, llm: Arc<dyn ChatProvider>) -> Self {
        Self {
            events: VecDeque::new(),
            store,
            llm,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Wit<Memory, Memory> for FondDuCoeur {
    /// Buffer completion or interruption events for later emotional
    /// evaluation. Other memory types are ignored.
    async fn observe(&mut self, input: Memory) {
        match input {
            Memory::Completion(_) | Memory::Interruption(_) => self.events.push_back(input),
            _ => {}
        }
    }

    /// Produce an [`Emotion`] based on the next buffered event using the
    /// provided [`ChatProvider`]. The resulting emotion is persisted via the
    /// [`MemoryStore`].
    async fn distill(&mut self) -> Option<Memory> {
        let event = self.events.pop_front()?;
        let reason = self.llm.evaluate_emotion(&event).await.ok()?;
        let mood = reason
            .split_whitespace()
            .skip_while(|w| *w != "feel")
            .nth(1)
            .unwrap_or("neutral")
            .to_string();

        let emotion = Emotion {
            uuid: Uuid::new_v4(),
            subject: event.uuid(),
            mood,
            reason,
            timestamp: SystemTime::now(),
        };

        info!("💓 Pete feels: {}", emotion.reason);
        let mem = Memory::Of(Box::new(emotion));
        let _ = self.store.save(&mem).await;
        Some(mem)
    }
}
