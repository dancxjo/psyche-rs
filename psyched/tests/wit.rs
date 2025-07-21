use tempfile::tempdir;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn wit_produces_output() {
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
        "[pipeline]\n\n[wit.echo]\ninput = \"sensation/chat\"\noutput = \"reply\"\nprompt = \"Respond\"\npriority = 0\nfeedback = \"\"\n",
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
            // directly append a sensation instead of using the socket
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

            let path = soul_dir.join("memory/reply.jsonl");
            let content = tokio::fs::read_to_string(&path).await.unwrap();
            let lines: Vec<_> = content.lines().collect();
            assert_eq!(lines.len(), 1);
            let entry: psyche::models::MemoryEntry = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(entry.how, "mock response");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn feedback_forwards_output() {
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
    let config = "\
[pipeline]\n\n[wit.first]\ninput = \"sensation/chat\"\noutput = \"reply1\"\nprompt = \"Respond\"\npriority = 0\nfeedback = \"second\"\n\n[wit.second]\ninput = \"reply1\"\noutput = \"reply2\"\nprompt = \"Respond2\"\npriority = 0\n";
    tokio::fs::write(&config_path, config).await.unwrap();

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
                text: "hi".into(),
            };
            let line = serde_json::to_string(&sens).unwrap();
            tokio::fs::write(&memory_path, format!("{}\n", line))
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();

            let path = soul_dir.join("memory/reply2.jsonl");
            let content = tokio::fs::read_to_string(&path).await.unwrap();
            let lines: Vec<_> = content.lines().collect();
            assert_eq!(lines.len(), 1);
        })
        .await;
}
