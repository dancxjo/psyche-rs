#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    psyche_rs::pete::launch_default_pete().await
}
