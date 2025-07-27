use clap::Parser;
use daemon_common::{LogLevel, maybe_daemonize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "whisperd", about = "Audio ingestion and transcription daemon")]
struct Cli {
    /// Path to the output socket
    #[arg(long, default_value = "/run/quick.sock")]
    socket: PathBuf,

    /// Path to the input socket to listen for PCM audio
    #[arg(long)]
    listen: PathBuf,

    /// Path to whisper model
    #[arg(long, env = "WHISPER_MODEL")]
    whisper_model: PathBuf,

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
        // .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();
    maybe_daemonize(cli.daemon)?;
    whisperd::run(cli.socket, cli.listen, cli.whisper_model).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_info_log_level() {
        let cli = Cli::try_parse_from([
            "whisperd",
            "--listen",
            "in.sock",
            "--whisper-model",
            "model.bin",
        ])
        .unwrap();
        assert!(matches!(cli.log_level, LogLevel::Info));
    }

    #[test]
    fn parses_debug_log_level() {
        let cli = Cli::try_parse_from([
            "whisperd",
            "--listen",
            "in.sock",
            "--whisper-model",
            "model.bin",
            "--log-level",
            "debug",
        ])
        .unwrap();
        assert!(matches!(cli.log_level, LogLevel::Debug));
    }
}
