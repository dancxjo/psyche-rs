use neo4rs::Graph;
use psyche_rs::{
    MemoryStore, Neo4jStore, countenance::DummyCountenance, llm::DummyLLM, memory::Sensation,
    mouth::DummyMouth, pete,
};
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
    let llm = Arc::new(DummyLLM);
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
