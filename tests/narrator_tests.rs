use psyche_rs::{
    llm::LLMClient,
    memory::{Impression, Memory, MemoryStore},
    narrator::Narrator,
    store::embedding_store::{MemoryRetriever, simple_embed},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use uuid::Uuid;

struct MockStore {
    data: Arc<Mutex<HashMap<Uuid, Memory>>>,
}
impl MockStore {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
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
        Ok(vec![])
    }
    async fn of_type(&self, _t: &str, _l: usize) -> anyhow::Result<Vec<Memory>> {
        Ok(vec![])
    }
    async fn recent_since(&self, since: SystemTime) -> anyhow::Result<Vec<Memory>> {
        let mut values: Vec<_> = self
            .data
            .lock()
            .await
            .values()
            .cloned()
            .filter(|m| m.timestamp().unwrap_or(SystemTime::UNIX_EPOCH) > since)
            .collect();
        values.sort_by_key(|m| m.timestamp().unwrap());
        Ok(values)
    }
    async fn impressions_containing(&self, keyword: &str) -> anyhow::Result<Vec<Impression>> {
        let mut items: Vec<_> = self
            .data
            .lock()
            .await
            .values()
            .filter_map(|m| match m {
                Memory::Impression(i) if i.how.to_lowercase().contains(&keyword.to_lowercase()) => {
                    Some(i.clone())
                }
                _ => None,
            })
            .collect();
        items.sort_by_key(|i| i.timestamp);
        Ok(items)
    }
    async fn complete_intention(&self, _: Uuid, _: psyche_rs::Completion) -> anyhow::Result<()> {
        Ok(())
    }
    async fn interrupt_intention(&self, _: Uuid, _: psyche_rs::Interruption) -> anyhow::Result<()> {
        Ok(())
    }
}

struct EchoLLM;
#[async_trait::async_trait]
impl LLMClient for EchoLLM {
    async fn summarize(&self, _input: &[psyche_rs::Sensation]) -> anyhow::Result<String> {
        Ok(String::new())
    }
    async fn summarize_impressions(&self, items: &[Impression]) -> anyhow::Result<String> {
        Ok(items
            .iter()
            .map(|i| i.how.clone())
            .collect::<Vec<_>>()
            .join(" "))
    }
    async fn suggest_urges(&self, _imp: &Impression) -> anyhow::Result<Vec<psyche_rs::Urge>> {
        Ok(vec![])
    }
    async fn evaluate_emotion(&self, _event: &Memory) -> anyhow::Result<String> {
        Ok(String::new())
    }
}

fn make_impression(how: &str, ts: SystemTime) -> Impression {
    Impression {
        uuid: Uuid::new_v4(),
        how: how.into(),
        topic: "test".into(),
        composed_of: vec![],
        timestamp: ts,
    }
}

struct NaiveRetriever {
    entries: tokio::sync::Mutex<Vec<(Uuid, Vec<f32>)>>,
}

impl NaiveRetriever {
    fn new() -> Self {
        Self {
            entries: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    async fn insert(&self, id: Uuid, text: &str) {
        self.entries.lock().await.push((id, simple_embed(text)));
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
impl MemoryRetriever for NaiveRetriever {
    async fn find_similar(&self, text: &str, top_k: usize) -> anyhow::Result<Vec<Uuid>> {
        let q = simple_embed(text);
        let entries = self.entries.lock().await;
        let mut scored: Vec<_> = entries.iter().map(|(id, v)| (*id, cosine(&q, v))).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(scored.into_iter().take(top_k).map(|(id, _)| id).collect())
    }
}

#[tokio::test]
async fn narrate_since_returns_story() -> anyhow::Result<()> {
    let store = Arc::new(MockStore::new());
    let llm = Arc::new(EchoLLM);
    let retriever = Arc::new(NaiveRetriever::new());
    let narrator = Narrator {
        store: store.clone(),
        llm,
        retriever: retriever.clone(),
    };

    let now = SystemTime::now();
    let earlier = now - Duration::from_secs(600);

    for text in ["saw a bird", "ate lunch", "took a nap"] {
        let imp = make_impression(text, now);
        retriever.insert(imp.uuid, &imp.how).await;
        store.save(&Memory::Impression(imp)).await?;
    }

    let story = narrator.narrate_since(earlier).await?;
    assert!(story.contains("saw a bird"));
    assert!(story.contains("ate lunch"));
    assert!(story.contains("took a nap"));
    Ok(())
}

#[tokio::test]
async fn narrate_topic_filters_keyword() -> anyhow::Result<()> {
    let store = Arc::new(MockStore::new());
    let llm = Arc::new(EchoLLM);
    let retriever = Arc::new(NaiveRetriever::new());
    let narrator = Narrator {
        store: store.clone(),
        llm,
        retriever: retriever.clone(),
    };

    let now = SystemTime::now();
    let imp1 = make_impression("watched a movie", now);
    retriever.insert(imp1.uuid, &imp1.how).await;
    store.save(&Memory::Impression(imp1)).await?;
    let imp2 = make_impression("read a book", now);
    retriever.insert(imp2.uuid, &imp2.how).await;
    store.save(&Memory::Impression(imp2)).await?;

    let story = narrator.narrate_topic("book").await?;
    assert!(!story.contains("movie"));
    assert!(story.contains("book"));
    Ok(())
}

#[tokio::test]
async fn recall_relevant_finds_best_matches() -> anyhow::Result<()> {
    let store = Arc::new(MockStore::new());
    let llm = Arc::new(EchoLLM);
    let retriever = Arc::new(NaiveRetriever::new());
    let narrator = Narrator {
        store: store.clone(),
        llm,
        retriever: retriever.clone(),
    };

    let now = SystemTime::now();
    let imp1 = make_impression("eat apple", now);
    retriever.insert(imp1.uuid, &imp1.how).await;
    store.save(&Memory::Impression(imp1)).await?;
    let imp2 = make_impression("go to store", now);
    retriever.insert(imp2.uuid, &imp2.how).await;
    store.save(&Memory::Impression(imp2)).await?;

    let story = narrator.recall_relevant("apple").await?;
    assert!(!story.is_empty());
    Ok(())
}
