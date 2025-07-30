use clap::Parser;
use daemon_common::{LogLevel, maybe_daemonize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "whisperd", about = "Audio ingestion and transcription daemon")]
struct Cli {
    /// Path to the Unix socket used for audio input and transcript output
    #[arg(long, default_value = "/run/psyched/ear.sock")]
    socket: PathBuf,

    /// Path to whisper model
    #[arg(long, env = "WHISPER_MODEL")]
    whisper_model: PathBuf,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    /// Milliseconds of silence before transcribing
    #[arg(long, default_value = "1000")]
    silence_ms: u64,

    /// Maximum milliseconds for a single segment
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,

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
    whisperd::run(
        cli.socket,
        cli.whisper_model,
        cli.silence_ms,
        cli.timeout_ms,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_info_log_level() {
        let cli = Cli::try_parse_from(["whisperd", "--whisper-model", "model.bin"]).unwrap();
        assert!(matches!(cli.log_level, LogLevel::Info));
        assert_eq!(cli.silence_ms, 1000);
        assert_eq!(cli.timeout_ms, 20000);
    }

    #[test]
    fn parses_debug_log_level() {
        let cli = Cli::try_parse_from([
            "whisperd",
            "--whisper-model",
            "model.bin",
            "--log-level",
            "debug",
            "--silence-ms",
            "1500",
            "--timeout-ms",
            "5000",
        ])
        .unwrap();
        assert!(matches!(cli.log_level, LogLevel::Debug));
        assert_eq!(cli.silence_ms, 1500);
        assert_eq!(cli.timeout_ms, 5000);
    }
}
