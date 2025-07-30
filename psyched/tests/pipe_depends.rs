use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn pipes_wait_for_dependencies() {
    let dir = tempdir().unwrap();
    let quick = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let ear = dir.path().join("ear.sock");
    let soul = dir.path().to_path_buf();
    tokio::fs::create_dir_all(soul.join("memory"))
        .await
        .unwrap();

    let config = format!(
        "[sensor.whisperd]\nenabled = false\nsocket = \"{}\"\n\
         [pipe.hearing]\nsocket = \"{}\"\npath = \"/hearing\"\ndepends_on = [\"whisperd\"]\n",
        ear.display(),
        ear.display()
    );
    let cfg_path = soul.join("identity.toml");
    tokio::fs::write(&cfg_path, config).await.unwrap();

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
        quick.clone(),
        soul.clone(),
        cfg_path.clone(),
        registry.clone(),
        profile.clone(),
        memory_sock.clone(),
        async move {
            let _ = rx.await;
        },
    ));

    local
        .run_until(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let listener = UnixListener::bind(&ear).unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"ping\n").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();
            let content = tokio::fs::read_to_string(soul.join("memory/sensation.jsonl"))
                .await
                .unwrap_or_default();
            assert!(content.contains("ping"));
        })
        .await;
}
