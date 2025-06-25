use neo4rs::Graph;
use psyche_rs::pete;
use psyche_rs::{MemoryStore, Neo4jStore, Will};
use std::sync::Arc;

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
    let _will = Will::new(store);
    pete::launch_default_pete().await
}
