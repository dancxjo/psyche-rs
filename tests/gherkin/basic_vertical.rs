use std::path::PathBuf;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use serde_json::json;

#[tokio::test]
async fn sensation_results_in_instant() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("quick.sock");
    let memory = dir.path().join("sensation.jsonl");

    let (tx, rx) = oneshot::channel();
    let server = tokio::spawn(psyched::run(socket.clone(), memory.clone(), rx));

    // wait for socket to exist
    for _ in 0..10 {
        if socket.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    let msg = b"/chat\nI feel lonely\n---\n";
    stream.write_all(msg).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tx.send(()).unwrap();
    server.await.unwrap().unwrap();

    let content = tokio::fs::read_to_string(&memory).await.unwrap();
    let lines: Vec<_> = content.lines().collect();
    assert_eq!(lines.len(), 1);
    let sensation: psyche::models::Sensation = serde_json::from_str(lines[0]).unwrap();

    let instant_path = memory.with_file_name("instant.jsonl");
    let icontent = tokio::fs::read_to_string(&instant_path).await.unwrap();
    let ilines: Vec<_> = icontent.lines().collect();
    assert_eq!(ilines.len(), 1);
    let instant: psyche::models::MemoryEntry = serde_json::from_str(ilines[0]).unwrap();
    assert_eq!(instant.what, serde_json::json!([sensation.id]));
    assert_eq!(instant.how, "The interlocutor feels lonely");
}
