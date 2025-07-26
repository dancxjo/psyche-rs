use std::path::PathBuf;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn quick_to_combobulator_generates_situation() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let soul_dir = dir.path().to_path_buf();
    let _memory_path = soul_dir.join("memory/sensation.jsonl");
    let config_path = soul_dir.join("identity.toml");
    tokio::fs::create_dir_all(soul_dir.join("memory"))
        .await
        .unwrap();
    tokio::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../all_souls/layka/identity.toml"),
        &config_path,
    )
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
    let instance = std::sync::Arc::new(psyche::llm::LlmInstance {
        name: "mock".into(),
        chat: std::sync::Arc::new(psyche::llm::mock_chat::MockChat::default()),
        profile: profile.clone(),
        semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    });
    let local = LocalSet::new();
    let mem_store = rememberd::FileStore::new(soul_dir.join("memory"));
    let mem_task = local.spawn_local(rememberd::run(memory_sock.clone(), mem_store));
    let server = local.spawn_local(psyched::run(
        socket.clone(),
        soul_dir.clone(),
        config_path,
        std::time::Duration::from_millis(50),
        registry.clone(),
        profile.clone(),
        vec![instance.clone()],
        memory_sock.clone(),
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
            let msg = b"/chat\nI feel lonely\n---\n";
            stream.write_all(msg).await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();
            mem_task.abort();

            let instant_path = soul_dir.join("memory/instant.jsonl");
            let icontent = tokio::fs::read_to_string(&instant_path).await.unwrap();
            let ilines: Vec<_> = icontent.lines().collect();
            assert_eq!(ilines.len(), 1);

            let situation_path = soul_dir.join("memory/situation.jsonl");
            let scontent = tokio::fs::read_to_string(&situation_path).await.unwrap();
            let slines: Vec<_> = scontent.lines().collect();
            assert_eq!(slines.len(), 1);
        })
        .await;
}
