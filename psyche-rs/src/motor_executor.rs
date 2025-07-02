use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};
use tracing::{debug, warn};

use crate::{AbortGuard, Intention, MemoryStore, Motor, StoredImpression, StoredSensation};

/// Central dispatcher for motor intentions.
///
/// Accepts [`Intention`] values and dispatches them to registered motors
/// using a small worker pool. Dropping the executor aborts all workers.
pub struct MotorExecutor {
    tx: mpsc::Sender<Intention>,
    _guards: Vec<AbortGuard>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
}

impl MotorExecutor {
    /// Create a new executor with the provided motors, worker count and queue
    /// capacity. Provide a [`MemoryStore`] to persist intentions and sensations.
    pub fn new(
        motors: Vec<Arc<dyn Motor + Send + Sync>>,
        workers: usize,
        capacity: usize,
        store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Intention>(capacity);
        let rx = Arc::new(Mutex::new(rx));
        let motors = Arc::new(motors);
        let store = store.clone();
        let mut guards = Vec::new();

        for _ in 0..workers.max(1) {
            let motors = motors.clone();
            let rx = rx.clone();
            let store = store.clone();
            let handle = tokio::spawn(async move {
                loop {
                    let next = {
                        let mut lock = rx.lock().await;
                        lock.recv().await
                    };
                    let Some(intention) = next else { break };

                    if let Some(motor) =
                        motors.iter().find(|m| m.name() == intention.assigned_motor)
                    {
                        debug!(target_motor = %motor.name(), "Executing motor");

                        if let Some(store) = &store {
                            let stored_imp = StoredImpression {
                                id: uuid::Uuid::new_v4().to_string(),
                                kind: "Intention".into(),
                                when: chrono::Utc::now(),
                                how: intention.to_string(),
                                sensation_ids: Vec::new(),
                                impression_ids: Vec::new(),
                            };
                            if let Err(e) = store.store_impression(&stored_imp) {
                                warn!(?e, "failed to store intention as impression");
                            }
                        }

                        match motor.perform(intention).await {
                            Ok(result) => {
                                if let Some(store) = &store {
                                    for s in result.sensations {
                                        match serde_json::to_string(&s.what) {
                                            Ok(data) => {
                                                let stored = StoredSensation {
                                                    id: uuid::Uuid::new_v4().to_string(),
                                                    kind: s.kind.clone(),
                                                    when: s.when.with_timezone(&chrono::Utc),
                                                    data,
                                                };
                                                if let Err(e) = store.store_sensation(&stored) {
                                                    warn!(?e, "failed to store motor sensation");
                                                }
                                            }
                                            Err(e) => warn!(?e, "failed to serialize sensation"),
                                        }
                                    }
                                }
                            }
                            Err(e) => warn!(?e, "Motor action failed"),
                        }
                    } else {
                        warn!(?intention, "No matching motor for intention");
                    }
                }
            });
            guards.push(AbortGuard::new(handle));
        }

        Self {
            tx,
            _guards: guards,
            store,
        }
    }

    /// Queue an intention for execution.
    pub fn spawn_intention(&self, intention: Intention) {
        if let Err(e) = self.tx.try_send(intention) {
            warn!(?e, "Motor queue full; dropping intention");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, ActionResult, InMemoryStore, MotorError, Sensation};
    use futures::stream::{self, StreamExt};
    use serde_json::Value;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CountMotor(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Motor for CountMotor {
        fn description(&self) -> &'static str {
            "counts"
        }
        fn name(&self) -> &'static str {
            "count"
        }
        async fn perform(&self, _i: Intention) -> Result<ActionResult, MotorError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ActionResult {
                sensations: Vec::new(),
                completed: true,
                completion: None,
                interruption: None,
            })
        }
    }

    fn sample_intention() -> Intention {
        let body = stream::empty().boxed();
        Intention::to(Action::new("count", Value::Null, body)).assign("count")
    }

    fn sensing_intention() -> Intention {
        let body = stream::empty().boxed();
        Intention::to(Action::new("sense", Value::Null, body)).assign("sense")
    }

    #[tokio::test]
    async fn dispatches_to_motor() {
        let counter = Arc::new(AtomicUsize::new(0));
        let motor = Arc::new(CountMotor(counter.clone()));
        let executor = MotorExecutor::new(vec![motor], 1, 4, None);
        executor.spawn_intention(sample_intention());
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    struct SensationMotor;

    #[async_trait::async_trait]
    impl Motor for SensationMotor {
        fn description(&self) -> &'static str {
            "sensates"
        }
        fn name(&self) -> &'static str {
            "sense"
        }
        async fn perform(&self, _i: Intention) -> Result<ActionResult, MotorError> {
            Ok(ActionResult {
                sensations: vec![Sensation {
                    kind: "test".into(),
                    when: chrono::Local::now(),
                    what: serde_json::Value::Null,
                    source: None,
                }],
                completed: true,
                completion: None,
                interruption: None,
            })
        }
    }

    #[tokio::test]
    async fn stores_intentions_and_sensations() {
        let store = Arc::new(InMemoryStore::new());
        let motor = Arc::new(SensationMotor);
        let executor = MotorExecutor::new(vec![motor], 1, 4, Some(store.clone()));
        executor.spawn_intention(sensing_intention());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(store.impression_count(), 1);
        assert_eq!(store.sensation_count(), 1);
    }
}
