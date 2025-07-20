#![cfg(feature = "qdrant")]

use psyche::memory::{ensure_collection, CollectionMaker};

struct MockClient {
    exists: bool,
    pub created: std::sync::Mutex<bool>,
}

#[async_trait::async_trait(?Send)]
impl CollectionMaker for MockClient {
    async fn collection_exists(&self, _name: &str) -> anyhow::Result<bool> {
        Ok(self.exists)
    }

    async fn create_memory_collection(&self, _dim: u64) -> anyhow::Result<()> {
        *self.created.lock().unwrap() = true;
        Ok(())
    }
}

#[tokio::test]
async fn creates_collection_when_missing() {
    let client = MockClient {
        exists: false,
        created: std::sync::Mutex::new(false),
    };
    ensure_collection(&client, 3).await.unwrap();
    assert!(*client.created.lock().unwrap());
}

#[tokio::test]
async fn skips_creation_when_present() {
    let client = MockClient {
        exists: true,
        created: std::sync::Mutex::new(false),
    };
    ensure_collection(&client, 3).await.unwrap();
    assert!(!*client.created.lock().unwrap());
}
