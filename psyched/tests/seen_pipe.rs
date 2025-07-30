use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn captions_from_eye_are_stored() {
    let dir = tempdir().unwrap();
    let quick = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let eye = dir.path().join("eye.sock");
    let soul = dir.path().to_path_buf();
    tokio::fs::create_dir_all(soul.join("memory"))
        .await
        .unwrap();

    let config = format!(
        "[sensor.seen]\nenabled = false\nsocket = \"{}\"\nlog_level = \"trace\"\n\
         [pipe.vision]\nsocket = \"{}\"\npath = \"/vision\"\n",
        eye.display(),
        eye.display()
    );
    let cfg_path = soul.join("identity.toml");
    tokio::fs::write(&cfg_path, config).await.unwrap();

    let listener = UnixListener::bind(&eye).unwrap();

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
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"a cat\n").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();
            let content = tokio::fs::read_to_string(soul.join("memory/sensation.jsonl"))
                .await
                .unwrap_or_default();
            assert!(content.contains("a cat"));
        })
        .await;
}
