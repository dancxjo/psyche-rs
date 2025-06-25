/// Pete's voice system used to craft text responses.
///
/// The voice maintains optional context about Pete's current mood in order to
/// flavour its next utterance.
///
/// # Examples
///
/// ```
/// use psyche_rs::voice::Voice;
///
/// let mut voice = Voice::default();
/// assert_eq!(voice.prompt(), "😐 You said: ...");
/// voice.update_mood("😊".to_string());
/// assert!(voice.prompt().starts_with("😊"));
/// ```
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use uuid::Uuid;

use crate::memory::{Memory, MemoryStore};
use crate::narrator::Narrator;

use crate::mouth::Mouth;

pub struct Voice {
    /// Latest emotional tone to express with the next prompt.
    pub current_mood: Option<String>,
    /// Module responsible for narrating past events.
    pub narrator: Narrator,
    /// Output system used to speak generated text.
    pub mouth: Arc<dyn Mouth>,
    /// Store used to persist spoken utterances.
    pub store: Arc<dyn MemoryStore>,
}

impl Voice {
    /// Create a new [`Voice`] bound to the provided narrator, mouth and memory store.
    pub fn new(narrator: Narrator, mouth: Arc<dyn Mouth>, store: Arc<dyn MemoryStore>) -> Self {
        Self {
            current_mood: None,
            narrator,
            mouth,
            store,
        }
    }

    /// Update the currently expressed mood.
    pub fn update_mood(&mut self, mood: String) {
        self.current_mood = Some(mood);
    }

    /// Compose a prompt incorporating the current mood.
    pub fn prompt(&self) -> String {
        let mood = self.current_mood.as_deref().unwrap_or("😐");
        format!("{} You said: ...", mood)
    }

    /// Respond to a memory oriented query by narrating recent events.
    ///
    /// Queries containing the word "today" trigger a summary of the last
    /// 12 hours. All other queries are treated as topic keywords.
    pub async fn answer_memory_query(&mut self, query: &str) -> anyhow::Result<()> {
        let summary = if query.contains("today") {
            self.narrator
                .narrate_since(SystemTime::now() - Duration::from_secs(3600 * 12))
                .await?
        } else {
            self.narrator.narrate_topic(query).await?
        };

        self.mouth.say(&summary).await?;
        self.store
            .save(&Memory::Sensation(crate::memory::Sensation {
                uuid: Uuid::new_v4(),
                kind: "text/plain".into(),
                from: "voice".into(),
                payload: json!({ "content": summary }),
                timestamp: SystemTime::now(),
            }))
            .await?;

        Ok(())
    }
}

/// [`Voice`] implementations used for `Default` that perform no actions.
struct NoopMouth;

#[async_trait::async_trait(?Send)]
impl Mouth for NoopMouth {
    async fn say(&self, _phrase: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NoopStore;

#[async_trait::async_trait]
impl MemoryStore for NoopStore {
    async fn save(&self, _memory: &Memory) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_by_uuid(&self, _uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(None)
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn of_type(&self, _t: &str, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn recent_since(&self, _since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn impressions_containing(
        &self,
        _keyword: &str,
    ) -> anyhow::Result<Vec<crate::Impression>> {
        Ok(Vec::new())
    }

    async fn complete_intention(&self, _id: Uuid, _c: crate::Completion) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(&self, _id: Uuid, _i: crate::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Default for Voice {
    fn default() -> Self {
        let store = Arc::new(NoopStore);
        let llm = Arc::new(crate::llm::DummyLLM);
        let narrator = Narrator {
            store: store.clone(),
            llm,
        };
        Self::new(narrator, Arc::new(NoopMouth), store)
    }
}
