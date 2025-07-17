use std::path::PathBuf;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn injection_returns_immediately() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let soul_dir = dir.path().to_path_buf();
    std::env::set_var("USE_MOCK_LLM", "1");
    let config_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/configs/sample.toml");
    tokio::fs::create_dir_all(soul_dir.join("config"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(soul_dir.join("memory"))
        .await
        .unwrap();
    tokio::fs::copy(&config_path, soul_dir.join("config/pipeline.toml"))
        .await
        .unwrap();

    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::mock_chat::MockChat::default()),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });
    let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![psyche::llm::LlmCapability::Chat],
    });

    let (tx, rx) = tokio::sync::oneshot::channel();
    let local = LocalSet::new();
    let server = local.spawn_local(psyched::run(
        socket.clone(),
        soul_dir.clone(),
        soul_dir.join("config/pipeline.toml"),
        std::time::Duration::from_millis(50),
        registry.clone(),
        profile.clone(),
        async move {
            let _ = rx.await;
        },
    ));

    local
        .run_until(async {
            for _ in 0..10 {
                if socket.exists() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            let mut stream = UnixStream::connect(&socket).await.unwrap();
            let msg = b"/chat\nHello\n---\n";
            stream.write_all(msg).await.unwrap();
            // Wait for server to close connection
            let mut buf = [0u8; 1];
            tokio::time::timeout(std::time::Duration::from_millis(30), stream.read(&mut buf))
                .await
                .expect("server did not close connection promptly")
                .unwrap();

            tx.send(()).unwrap();
            server.await.unwrap().unwrap();
        })
        .await;
}
