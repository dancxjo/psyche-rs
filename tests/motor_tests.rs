use psyche_rs::{
    Intention, Memory, MemoryStore, Urge,
    motor::{DummyMotor, MotorSystem},
    wit::Wit,
    wits::will::Will,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

struct MockStore {
    data: Arc<AsyncMutex<HashMap<Uuid, Memory>>>,
}

impl MockStore {
    fn new() -> Self {
        Self {
            data: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MemoryStore for MockStore {
    async fn save(&self, memory: &Memory) -> anyhow::Result<()> {
        self.data.lock().await.insert(memory.uuid(), memory.clone());
        Ok(())
    }

    async fn get_by_uuid(&self, uuid: Uuid) -> anyhow::Result<Option<Memory>> {
        Ok(self.data.lock().await.get(&uuid).cloned())
    }

    async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn of_type(&self, _type_name: &str, _limit: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(Vec::new())
    }

    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }

    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

fn example_urge() -> Urge {
    Urge {
        uuid: Uuid::new_v4(),
        source: Uuid::new_v4(),
        motor_name: "move_forward".into(),
        parameters: json!({"speed":0.5,"duration":3.0}),
        intensity: 1.0,
        timestamp: SystemTime::now(),
    }
}

#[tokio::test]
async fn will_invokes_dummy_motor() {
    let store = Arc::new(MockStore::new());
    struct RecordingMotor {
        inner: DummyMotor,
        log: Arc<AsyncMutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl MotorSystem for RecordingMotor {
        async fn invoke(&self, intention: &Intention) -> anyhow::Result<()> {
            self.inner.invoke(intention).await?;
            let msg = format!("<{} {:?}/>", intention.motor_name, intention.parameters);
            self.log.lock().await.push(msg);
            Ok(())
        }
    }

    let log = Arc::new(AsyncMutex::new(Vec::new()));
    let motor = Arc::new(RecordingMotor {
        inner: DummyMotor,
        log: log.clone(),
    });
    let mut will = Will::new(store, motor);

    let urge = example_urge();
    will.observe(urge.clone()).await;

    let intent = will.distill().await.expect("should produce intention");
    drop(will);
    let logged = log.lock().await.pop().expect("motor not invoked");
    assert_eq!(
        logged,
        format!("<{} {:?}/>", intent.motor_name, intent.parameters)
    );
}
