use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "heard", about = "Audio ingestion and transcription daemon")]
struct Cli {
    /// Path to the output socket
    #[arg(long, default_value = "/run/quick.sock")]
    socket: PathBuf,

    /// Path to the input socket to listen for PCM audio
    #[arg(long)]
    listen: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    heard::run(cli.socket, cli.listen).await
}
