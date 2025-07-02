use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};
use tracing::{debug, warn};

use crate::{AbortGuard, Intention, Motor};

/// Central dispatcher for motor intentions.
///
/// Accepts [`Intention`] values and dispatches them to registered motors
/// using a small worker pool. Dropping the executor aborts all workers.
pub struct MotorExecutor {
    tx: mpsc::Sender<Intention>,
    _guards: Vec<AbortGuard>,
}

impl MotorExecutor {
    /// Create a new executor with the provided motors, worker count and queue
    /// capacity.
    pub fn new(motors: Vec<Arc<dyn Motor + Send + Sync>>, workers: usize, capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel::<Intention>(capacity);
        let rx = Arc::new(Mutex::new(rx));
        let motors = Arc::new(motors);
        let mut guards = Vec::new();

        for _ in 0..workers.max(1) {
            let motors = motors.clone();
            let rx = rx.clone();
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
                        if let Err(e) = motor.perform(intention).await {
                            warn!(?e, "Motor action failed");
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
    use crate::{Action, ActionResult, MotorError};
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

    #[tokio::test]
    async fn dispatches_to_motor() {
        let counter = Arc::new(AtomicUsize::new(0));
        let motor = Arc::new(CountMotor(counter.clone()));
        let executor = MotorExecutor::new(vec![motor], 1, 4);
        executor.spawn_intention(sample_intention());
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
