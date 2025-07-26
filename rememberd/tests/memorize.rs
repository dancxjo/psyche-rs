use rememberd::{run, FileStore};
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test]
async fn memorize_appends_to_file() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("memory.sock");
    let store = FileStore::new(dir.path().join("mem"));
    tokio::fs::create_dir_all(&store.dir).await.unwrap();
    let rt = LocalSet::new();
    let handle = rt.spawn_local(run(sock.clone(), store.clone()));
    rt.run_until(async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut client = UnixStream::connect(&sock).await.unwrap();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "memorize",
            "params": {"kind": "instant", "data": {"foo": "bar"}},
            "id": 1
        });
        let data = serde_json::to_vec(&req).unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, &data)
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    })
    .await;
    handle.abort();
    let path = dir.path().join("mem/instant.jsonl");
    let content = tokio::fs::read_to_string(path).await.unwrap();
    let lines: Vec<_> = content.lines().collect();
    assert_eq!(lines.len(), 1);
}
