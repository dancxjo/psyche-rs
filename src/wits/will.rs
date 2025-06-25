use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use uuid::Uuid;

use crate::memory::{Intention, IntentionStatus, Memory, MemoryStore, Urge};
use crate::motor::{MotorFeedback, MotorSystem};
use crate::wit::Wit;

/// `Will` consumes [`Urge`]s and issues [`Intention`]s to the provided
/// [`MotorSystem`]. Observed urges are persisted via the supplied
/// [`MemoryStore`].
pub struct Will {
    buffer: VecDeque<Urge>,
    pub store: Arc<dyn MemoryStore>,
    pub motor: Arc<dyn MotorSystem>,
}

impl Will {
    /// Create a new [`Will`] with the given memory store and motor system.
    pub fn new(store: Arc<dyn MemoryStore>, motor: Arc<dyn MotorSystem>) -> Self {
        Self {
            buffer: VecDeque::new(),
            store,
            motor,
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

        // Invoke the motor and persist any feedback.
        match self.motor.invoke(&intent).await {
            Ok(MotorFeedback::Completed(done)) => {
                self.store.complete_intention(intent.uuid, done).await.ok();
            }
            Ok(MotorFeedback::Interrupted(breakoff)) => {
                self.store
                    .interrupt_intention(intent.uuid, breakoff)
                    .await
                    .ok();
            }
            Err(err) => {
                eprintln!("motor failed: {}", err);
            }
        }

        Some(intent)
    }
}
