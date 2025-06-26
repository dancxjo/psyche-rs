use llm::LLMProvider;
use llm::builder::{LLMBackend, LLMBuilder};
use neo4rs::Graph;
use psyche_rs::llm::LLMClient;
use psyche_rs::{
    MemoryStore, Neo4jStore, countenance::DummyCountenance, memory::Sensation, mouth::DummyMouth,
    pete,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::LocalSet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let uri = std::env::var("NEO4J_URI").unwrap_or_else(|_| "127.0.0.1:7687".into());
    let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let pass = std::env::var("NEO4J_PASS").unwrap_or_else(|_| "neo4j".into());

    let graph = Graph::new(&uri, &user, &pass)
        .await
        .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
    let store: Arc<dyn MemoryStore> = Arc::new(Neo4jStore { client: graph });
    let backend = std::env::var("LLM_BACKEND").unwrap_or_else(|_| "ollama".into());
    let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gemma3:27b".into());
    let base_url =
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".into());

    let llm_provider = LLMBuilder::new()
        .backend(LLMBackend::from_str(&backend)?)
        .api_key(api_key)
        .base_url(base_url)
        .model(model)
        .stream(true)
        .build()?;

    let llm = Arc::new(psyche_rs::llm::ChatLLM(Arc::<dyn LLMProvider>::from(
        llm_provider,
    ))) as Arc<dyn LLMClient>;
    let mouth = Arc::new(DummyMouth);
    let face = Arc::new(DummyCountenance);

    let (psyche, tx, _stop_rx) = pete::build_pete(store, llm, mouth, face);

    let local = LocalSet::new();
    local.spawn_local(async move { psyche.tick().await });

    local
        .run_until(async move {
            for i in 0..3 {
                tx.send(Sensation::new_text(format!("This is test {}", i), "cli"))
                    .await?;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await?;

    Ok(())
}
