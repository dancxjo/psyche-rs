use std::path::PathBuf;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let socket = PathBuf::from("/run/quick.sock");
    let memory = PathBuf::from("soul/memory");
    let (_tx, rx) = oneshot::channel();
    psyched::run(socket, memory, rx).await
}
