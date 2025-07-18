#[cfg(feature = "neo4j")]
use async_trait::async_trait;
#[cfg(feature = "neo4j")]
use psyche::memory::{context_subgraph, Experience, MemoryBackend};

#[cfg(feature = "neo4j")]
struct SpyBackend {
    last: std::sync::Mutex<Option<String>>,
}

#[cfg(feature = "neo4j")]
#[async_trait(?Send)]
impl MemoryBackend for SpyBackend {
    async fn store(&self, _exp: &Experience, _vector: &[f32]) -> anyhow::Result<String> {
        Ok("1".into())
    }
    async fn search(&self, _vector: &[f32], _top_k: usize) -> anyhow::Result<Vec<Experience>> {
        Ok(vec![])
    }
    async fn get(&self, _id: &str) -> anyhow::Result<Option<Experience>> {
        Ok(None)
    }
    async fn cypher_query(&self, query: &str) -> anyhow::Result<Vec<Experience>> {
        *self.last.lock().unwrap() = Some(query.to_string());
        Ok(vec![])
    }

    async fn link_summary(&self, _summary_id: &str, _original_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "neo4j")]
#[tokio::test]
async fn builds_context_query() {
    let backend = SpyBackend {
        last: std::sync::Mutex::new(None),
    };
    let _ = context_subgraph(&backend, "abc").await.unwrap();
    let q = backend.last.lock().unwrap().clone().unwrap();
    assert!(q.contains("MATCH (e:Experience {id: \"abc\"})"));
}
