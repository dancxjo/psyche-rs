use tempfile::tempdir;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn run_without_backends() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let soul_dir = dir.path().to_path_buf();
    tokio::fs::create_dir_all(soul_dir.join("memory"))
        .await
        .unwrap();
    tokio::fs::write(
        soul_dir.join("identity.toml"),
        "[wit.dummy]\ninput = \"foo\"\noutput = \"bar\"\nprompt = \"p\"",
    )
    .await
    .unwrap();

    std::env::set_var("QDRANT_URL", "http://127.0.0.1:65535");
    std::env::set_var("NEO4J_URL", "bolt://127.0.0.1:65535");

    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::mock_chat::MockChat::default()),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });
    let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![psyche::llm::LlmCapability::Chat],
    });
    let local = LocalSet::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let server = local.spawn_local(psyched::run(
        socket.clone(),
        soul_dir.clone(),
        soul_dir.join("identity.toml"),
        registry.clone(),
        profile.clone(),
        memory_sock.clone(),
        async move {
            let _ = rx.await;
        },
    ));

    local
        .run_until(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();
        })
        .await;
}
