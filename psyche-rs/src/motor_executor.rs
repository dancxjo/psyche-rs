use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, warn};

use crate::{
    AbortGuard, Intention, MemoryStore, Motor, RecentActionsLog, StoredImpression, StoredSensation,
};

/// Central dispatcher for motor intentions.
///
/// Accepts [`Intention`] values and dispatches them to registered motors
/// using a small worker pool. Dropping the executor aborts all workers.
pub struct MotorExecutor {
    tx: mpsc::Sender<Intention>,
    _guards: Vec<AbortGuard>,
    #[allow(dead_code)]
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    #[allow(dead_code)]
    actions: Option<Arc<RecentActionsLog>>,
}

impl MotorExecutor {
    /// Create a new executor with the provided motors, worker count and queue
    /// capacity. Provide a [`MemoryStore`] to persist intentions and sensations.
    pub fn new(
        motors: Vec<Arc<dyn Motor + Send + Sync>>,
        workers: usize,
        capacity: usize,
        store: Option<Arc<dyn MemoryStore + Send + Sync>>,
        actions: Option<Arc<RecentActionsLog>>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<Intention>(capacity);
        let motors = Arc::new(motors);
        let semaphore = Arc::new(Semaphore::new(workers.max(1)));
        let store_cloned = store.clone();
        let actions_cloned = actions.clone();
        let handle = tokio::spawn({
            let motors = motors.clone();
            let semaphore = semaphore.clone();
            let actions = actions_cloned;
            async move {
                while let Some(intention) = rx.recv().await {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let motors = motors.clone();
                    let store = store_cloned.clone();
                    let actions = actions.clone();
                    tokio::spawn(async move {
                        let summary = intention.completed_summary();
                        if let Some(motor) =
                            motors.iter().find(|m| m.name() == intention.assigned_motor)
                        {
                            debug!(target_motor = %motor.name(), "Executing motor");
                            if let Some(store) = &store {
                                let stored_imp = StoredImpression {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    kind: "Intention".into(),
                                    when: chrono::Utc::now(),
                                    how: intention.summary(),
                                    sensation_ids: Vec::new(),
                                    impression_ids: Vec::new(),
                                };
                                if let Err(e) = store.store_impression(&stored_imp).await {
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
                                                    if let Err(e) =
                                                        store.store_sensation(&stored).await
                                                    {
                                                        warn!(
                                                            ?e,
                                                            "failed to store motor sensation"
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!(?e, "failed to serialize sensation")
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => warn!(?e, "Motor action failed"),
                            }
                        } else {
                            warn!(?intention, "No matching motor for intention");
                        }
                        if let Some(log) = &actions {
                            log.push(summary);
                        }
                        drop(permit);
                    });
                }
            }
        });
        let mut guards = Vec::new();
        guards.push(AbortGuard::new(handle));

        Self {
            tx,
            _guards: guards,
            store,
            actions,
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

    fn delay_intention() -> Intention {
        let body = stream::empty().boxed();
        Intention::to(Action::new("delay", Value::Null, body)).assign("delay")
    }

    fn sensing_intention() -> Intention {
        let body = stream::empty().boxed();
        Intention::to(Action::new("sense", Value::Null, body)).assign("sense")
    }

    #[tokio::test]
    async fn dispatches_to_motor() {
        let counter = Arc::new(AtomicUsize::new(0));
        let motor = Arc::new(CountMotor(counter.clone()));
        let executor = MotorExecutor::new(vec![motor], 1, 4, None, None);
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
        let executor = MotorExecutor::new(vec![motor], 1, 4, Some(store.clone()), None);
        executor.spawn_intention(sensing_intention());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(store.impression_count(), 1);
        assert_eq!(store.sensation_count(), 1);
    }

    struct DelayMotor(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Motor for DelayMotor {
        fn description(&self) -> &'static str {
            "delays"
        }
        fn name(&self) -> &'static str {
            "delay"
        }
        async fn perform(&self, _i: Intention) -> Result<ActionResult, MotorError> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ActionResult {
                sensations: Vec::new(),
                completed: true,
                completion: None,
                interruption: None,
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_workers_process_tasks() {
        let counter = Arc::new(AtomicUsize::new(0));
        let motor = Arc::new(DelayMotor(counter.clone()));
        let executor = MotorExecutor::new(vec![motor], 2, 4, None, None);
        executor.spawn_intention(delay_intention());
        executor.spawn_intention(delay_intention());
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
