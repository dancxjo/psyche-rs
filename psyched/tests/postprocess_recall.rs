use tempfile::tempdir;
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn wit_recall_postprocess_sends_query() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory_sock = dir.path().join("memory.sock");
    let soul_dir = dir.path().to_path_buf();
    let memory_path = soul_dir.join("memory/sensation.jsonl");
    tokio::fs::create_dir_all(soul_dir.join("config"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(soul_dir.join("memory"))
        .await
        .unwrap();
    let config_path = soul_dir.join("config/pipeline.toml");
    tokio::fs::write(
        &config_path,
        "[pipeline]\n\n[wit.echo]\ninput = \"sensation/chat\"\noutput = \"reply\"\nprompt = \"Respond\"\npriority = 0\npostprocess = \"recall\"\n",
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
    let instance = std::sync::Arc::new(psyche::llm::LlmInstance {
        name: "mock".into(),
        chat: std::sync::Arc::new(psyche::llm::mock_chat::MockChat::default()),
        profile: profile.clone(),
        semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    });

    let listener = UnixListener::bind(&memory_sock).unwrap();
    let recv = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).await.unwrap();
        buf
    });

    let (tx, rx) = tokio::sync::oneshot::channel();
    let local = LocalSet::new();
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
            let sens = psyche::models::Sensation {
                id: uuid::Uuid::new_v4().to_string(),
                path: "/chat".into(),
                text: "hello".into(),
            };
            let line = serde_json::to_string(&sens).unwrap();
            tokio::fs::write(&memory_path, format!("{}\n", line))
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();

            let msg = recv.await.unwrap();
            assert!(msg.starts_with("/recall"));
            assert!(msg.contains("mock response"));
        })
        .await;
}
