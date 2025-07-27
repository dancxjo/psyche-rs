use rememberd::{run, FileStore};
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test]
async fn recall_policy_creates_recall_entry() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("memory.sock");
    let mem_dir = dir.path().join("mem");
    tokio::fs::create_dir_all(&mem_dir).await.unwrap();
    // enable recall for instants
    tokio::fs::write(
        mem_dir.join("policy.toml"),
        "[recall]\nkinds = [\"instant\"]\n",
    )
    .await
    .unwrap();
    let store = FileStore::new(mem_dir.clone());
    let rt = LocalSet::new();
    let handle = rt.spawn_local(run(sock.clone(), store.clone()));
    rt.run_until(async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut client = UnixStream::connect(&sock).await.unwrap();
        let entry = serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "kind": "instant",
            "when": chrono::Utc::now(),
            "what": [],
            "how": "something happened",
        });
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "memorize",
            "params": {"kind": "instant", "data": entry},
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
    let recall_path = mem_dir.join("recall.jsonl");
    let content = tokio::fs::read_to_string(recall_path).await.unwrap();
    assert_eq!(content.lines().count(), 1);
}
