use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info};

mod policy;
mod rpc;
mod store;

pub use store::FileStore;

async fn handle_connection(stream: UnixStream, store: FileStore) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).await?;
    if buf.is_empty() {
        return Ok(());
    }
    let req: rpc::RpcRequest = serde_json::from_slice(&buf)?;
    let resp = rpc::dispatch(req, &store).await?;
    let msg = serde_json::to_vec(&resp)?;
    let mut stream = reader.into_inner();
    stream.write_all(&msg).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Run the `rememberd` JSON-RPC server.
pub async fn run(socket: PathBuf, store: FileStore) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "rememberd listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let st = store.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, st).await {
                error!(error = %e, "connection failed");
            }
        });
    }
}
