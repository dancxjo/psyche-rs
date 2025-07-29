use std::path::PathBuf;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn sensation_results_in_instant() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let soul_dir = dir.path().to_path_buf();
    let memory_path = soul_dir.join("memory/sensation.jsonl");
    let config_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/configs/sample.toml");
    tokio::fs::create_dir_all(soul_dir.join("memory"))
        .await
        .unwrap();
    std::env::set_var("USE_MOCK_LLM", "1");
    tokio::fs::copy(config_path, soul_dir.join("identity.toml"))
        .await
        .unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let local = LocalSet::new();
    let registry = std::sync::Arc::new(psyche::llm::LlmRegistry {
        chat: Box::new(psyche::llm::mock_chat::MockChat::default()),
        embed: Box::new(psyche::llm::mock_embed::MockEmbed::default()),
    });
    let profile = std::sync::Arc::new(psyche::llm::LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![psyche::llm::LlmCapability::Chat],
    });
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
            // wait for socket to exist
            for _ in 0..10 {
                if socket.exists() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            let mut stream = UnixStream::connect(&socket).await.unwrap();
            let msg = b"/chat\nI feel lonely\n---\n";
            stream.write_all(msg).await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = tx.send(());
            server.await.unwrap().unwrap();

            let sensation_path = memory_path.clone();
            let content = tokio::fs::read_to_string(&sensation_path).await.unwrap();
            let lines: Vec<_> = content.lines().collect();
            assert_eq!(lines.len(), 1);
            let sensation: psyche::models::Sensation = serde_json::from_str(lines[0]).unwrap();

            // only the sensation should be stored without wits running
        })
        .await;
}

#[tokio::test]
async fn cli_flags_work() {
    let exe = env!("CARGO_BIN_EXE_psyched");
    let status = tokio::process::Command::new(exe)
        .arg("--version")
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let status = tokio::process::Command::new(exe)
        .arg("--soul")
        .arg("/tmp/foo")
        .arg("--identity")
        .arg("/tmp/bar")
        .arg("--log-level")
        .arg("debug")
        .arg("--qdrant-url")
        .arg("http://localhost:6333")
        .arg("--neo4j-url")
        .arg("bolt://localhost:7687")
        .arg("--neo4j-user")
        .arg("neo4j")
        .arg("--neo4j-pass")
        .arg("password")
        .arg("--help")
        .status()
        .await
        .unwrap();
    assert!(status.success());
}
