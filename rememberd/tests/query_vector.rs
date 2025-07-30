use rememberd::{run, FileStore};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::task::LocalSet;

#[tokio::test]
async fn query_vector_without_qdrant_returns_empty() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("memory.sock");
    let mem_dir = dir.path().join("mem");
    tokio::fs::create_dir_all(&mem_dir).await.unwrap();
    let store = FileStore::new(mem_dir.clone());
    let rt = LocalSet::new();
    let handle = rt.spawn_local(run(sock.clone(), store.clone()));
    rt.run_until(async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut client = UnixStream::connect(&sock).await.unwrap();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "query_vector",
            "params": {"kind": "face", "vector": [0.0, 0.0, 0.0], "top_k": 1},
            "id": 1
        });
        let data = serde_json::to_vec(&req).unwrap();
        client.write_all(&data).await.unwrap();
        client.shutdown().await.unwrap();
        let mut buf = Vec::new();
        tokio::io::BufReader::new(client)
            .read_to_end(&mut buf)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(resp["result"].as_array().unwrap().len(), 0);
    })
    .await;
    handle.abort();
}
