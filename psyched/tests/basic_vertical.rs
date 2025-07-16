use std::path::PathBuf;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn sensation_results_in_instant() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory_path = dir.path().join("sensation.jsonl");
    let memory_dir = dir.path().to_path_buf();
    let config_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/configs/sample.toml");
    tokio::fs::copy(config_path, memory_dir.join("psyche.toml"))
        .await
        .unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let local = LocalSet::new();
    let server = local.spawn_local(psyched::run(
        socket.clone(),
        memory_path.clone(),
        memory_dir.join("psyche.toml"),
        std::time::Duration::from_millis(50),
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
            tx.send(()).unwrap();
            server.await.unwrap().unwrap();

            let sensation_path = memory_path.clone();
            let content = tokio::fs::read_to_string(&sensation_path).await.unwrap();
            let lines: Vec<_> = content.lines().collect();
            assert_eq!(lines.len(), 1);
            let sensation: psyche::models::Sensation = serde_json::from_str(lines[0]).unwrap();

            let instant_path = memory_dir.join("instant.jsonl");
            let icontent = tokio::fs::read_to_string(&instant_path).await.unwrap();
            let ilines: Vec<_> = icontent.lines().collect();
            assert_eq!(ilines.len(), 1);
            let instant: psyche::models::MemoryEntry = serde_json::from_str(ilines[0]).unwrap();
            assert_eq!(instant.what, serde_json::json!([sensation.id]));
            assert_eq!(instant.how, "The interlocutor feels lonely");

            let situation_path = memory_dir.join("situation.jsonl");
            let scontent = tokio::fs::read_to_string(&situation_path).await.unwrap();
            let slines: Vec<_> = scontent.lines().collect();
            assert_eq!(slines.len(), 1);
            let situation: psyche::models::MemoryEntry = serde_json::from_str(slines[0]).unwrap();
            assert_eq!(situation.kind, "situation");
            assert!(!situation.how.is_empty());
            assert_eq!(situation.what, serde_json::json!([instant.id]));
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
        .arg("--help")
        .status()
        .await
        .unwrap();
    assert!(status.success());
}
