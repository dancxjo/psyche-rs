use std::sync::Arc;

use crate::{Memory, MemoryStore};

/// The `Will` is responsible for remembering [`Memory`] values by writing them
/// into a shared [`MemoryStore`].
///
/// ```no_run
/// use psyche_rs::{Will, MemoryStore, Memory, Sensation, Completion, Interruption};
/// use std::sync::Arc;
/// use serde_json::json;
/// use std::time::SystemTime;
/// use uuid::Uuid;
///
/// struct DummyStore;
/// #[async_trait::async_trait]
/// impl MemoryStore for DummyStore {
///     async fn save(&self, _m: &Memory) -> anyhow::Result<()> { Ok(()) }
///     async fn get_by_uuid(&self, _u: Uuid) -> anyhow::Result<Option<Memory>> { Ok(None) }
///     async fn recent(&self, _l: usize) -> anyhow::Result<Vec<Memory>> { Ok(vec![]) }
///     async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> { Ok(vec![]) }
///     async fn complete_intention(&self, _:Uuid, _:Completion) -> anyhow::Result<()> { Ok(()) }
///     async fn interrupt_intention(&self, _:Uuid, _:Interruption) -> anyhow::Result<()> { Ok(()) }
/// }
///
/// # async fn run() -> anyhow::Result<()> {
/// let store = Arc::new(DummyStore) as Arc<dyn MemoryStore>;
/// let will = Will::new(store);
/// let mem = Memory::Sensation(Sensation {
///     uuid: Uuid::new_v4(),
///     kind: "ping".into(),
///     from: "doctest".into(),
///     payload: json!({"a":1}),
///     timestamp: SystemTime::now(),
/// });
/// will.remember(mem).await?;
/// # Ok(()) }
/// ```
pub struct Will {
    store: Arc<dyn MemoryStore>,
}

impl Will {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    pub async fn remember(&self, memory: Memory) -> anyhow::Result<()> {
        self.store.save(&memory).await
    }
}
