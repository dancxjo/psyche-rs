use clap::Parser;
use daemon_common::{LogLevel, maybe_daemonize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "spoken", about = "Text-to-speech daemon")]
struct Cli {
    /// Path to the Unix socket
    #[arg(long, default_value = "/run/psyched/voice.sock")]
    socket: PathBuf,

    /// Base URL of the coqui TTS server
    #[arg(long, default_value = "http://localhost:5002")]
    tts_url: String,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    /// Run as a background daemon
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
    spoken::run(cli.socket, cli.tts_url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults() {
        let cli = Cli::try_parse_from(["spoken"]).unwrap();
        assert!(matches!(cli.log_level, LogLevel::Info));
        assert_eq!(cli.tts_url, "http://localhost:5002");
    }
}
