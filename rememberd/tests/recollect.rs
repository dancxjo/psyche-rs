use psyche::llm::mock_embed::MockEmbed;
use psyche::llm::prompt::PromptHelper;
use psyche::llm::{LlmCapability, LlmProfile};
use psyche::memory::{InMemoryBackend, Memorizer};
use rememberd::run;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

#[tokio::test]
async fn recalls_memory_and_emits() {
    let dir = tempdir().unwrap();
    let in_sock = dir.path().join("memory.sock");
    let out_sock = dir.path().join("quick.sock");

    let backend = InMemoryBackend::default();
    let profile = LlmProfile {
        provider: "mock".into(),
        model: "mock".into(),
        capabilities: vec![LlmCapability::Embedding],
    };
    let embed = MockEmbed::default();
    let memorizer = Memorizer {
        chat: None,
        embed: &embed,
        profile: &profile,
        backend: &backend,
        prompter: PromptHelper::default(),
    };
    memorizer
        .memorize("I was alone on Mars", Some("being alone"), false, vec![])
        .await
        .unwrap();

    // listener for output
    let listener = UnixListener::bind(&out_sock).unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).await.unwrap();
        buf
    });

    let rt = tokio::task::LocalSet::new();
    let out_clone = out_sock.clone();
    let in_sock_clone = in_sock.clone();
    let handle = rt.spawn_local(async move {
        run(in_sock_clone, out_clone, backend, Box::new(embed), profile)
            .await
            .unwrap();
    });

    rt.run_until(async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut c = UnixStream::connect(&in_sock).await.unwrap();
        c.write_all(b"/recall\nlonely?\n\n").await.unwrap();
        drop(c);
        let data = server.await.unwrap();
        assert!(data.contains("/memory/recalled"));
    })
    .await;
    handle.abort();
}
