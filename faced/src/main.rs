use clap::Parser;
use daemon_common::{LogLevel, maybe_daemonize};
use faced::opencv_recognizer::OpenCVRecognizer;
use faced::{Recognizer, run};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "faced", about = "Face recognition daemon")]
struct Cli {
    #[arg(long, default_value = "/run/psyche/faced.sock")]
    socket: PathBuf,

    #[arg(long, default_value = "/run/psyche/rememberd.sock")]
    memory_socket: PathBuf,

    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    #[arg(short = 'd', long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();
    maybe_daemonize(cli.daemon)?;
    let rec = Arc::new(OpenCVRecognizer::new(cli.memory_socket));
    run(cli.socket, rec).await
}
