use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use uuid::Uuid;

use crate::memory::{Intention, IntentionStatus, Memory, MemoryStore, Urge};

use crate::wit::Wit;
use tracing::info;

/// `Will` consumes [`Urge`]s, persists them and emits [`Intention`]s.
/// Motor execution is handled elsewhere.
pub struct Will {
    buffer: VecDeque<Urge>,
    pub store: Arc<dyn MemoryStore>,
}

impl Will {
    /// Create a new [`Will`] with the given memory store.
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self {
            buffer: VecDeque::new(),
            store,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Wit<Urge, Intention> for Will {
    /// Buffer the urge and persist it to the [`MemoryStore`].
    async fn observe(&mut self, input: Urge) {
        let _ = self.store.save(&Memory::Urge(input.clone())).await;
        self.buffer.push_back(input);
    }

    /// Convert the next buffered [`Urge`] into an [`Intention`], invoke the
    /// associated motor command, and persist the intention.
    async fn distill(&mut self) -> Option<Intention> {
        let urge = self.buffer.pop_front()?;
        let intent = Intention {
            uuid: Uuid::new_v4(),
            urge: urge.uuid,
            motor_name: urge.motor_name,
            parameters: urge.parameters,
            issued_at: SystemTime::now(),
            resolved_at: None,
            status: IntentionStatus::Pending,
        };

        let _ = self.store.save(&Memory::Intention(intent.clone())).await;

        info!("🎯 Pete intends: {}", intent.motor_name);

        Some(intent)
    }
}
