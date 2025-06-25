//! Standalone binary launching a simple Pete instance.

use psyche_rs::pete;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    pete::launch_default_pete().await
}
