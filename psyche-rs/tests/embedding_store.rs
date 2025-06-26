use psyche_rs::store::embedding_store::{MemoryRetriever, simple_embed};
use uuid::Uuid;

struct NaiveStore {
    entries: Vec<(Uuid, Vec<f32>)>,
}

impl NaiveStore {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    async fn insert(&mut self, id: Uuid, vec: Vec<f32>) {
        self.entries.push((id, vec));
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[async_trait::async_trait(?Send)]
impl MemoryRetriever for NaiveStore {
    async fn find_similar(&self, text: &str, top_k: usize) -> anyhow::Result<Vec<Uuid>> {
        let q = simple_embed(text);
        let mut scored: Vec<_> = self
            .entries
            .iter()
            .map(|(id, v)| (*id, cosine(&q, v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(scored.into_iter().take(top_k).map(|(id, _)| id).collect())
    }
}

#[tokio::test]
async fn retrieves_most_similar() -> anyhow::Result<()> {
    let mut store = NaiveStore::new();
    let hello_id = Uuid::new_v4();
    store.insert(hello_id, simple_embed("hello world")).await;
    let bye_id = Uuid::new_v4();
    store.insert(bye_id, simple_embed("good bye")).await;

    let result = store.find_similar("good bye", 1).await?;
    assert_eq!(result, vec![bye_id]);
    Ok(())
}
