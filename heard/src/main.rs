use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl From<LogLevel> for tracing_subscriber::filter::LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing_subscriber::filter::LevelFilter::ERROR,
            LogLevel::Warn => tracing_subscriber::filter::LevelFilter::WARN,
            LogLevel::Info => tracing_subscriber::filter::LevelFilter::INFO,
            LogLevel::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
            LogLevel::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "heard", about = "Audio ingestion and transcription daemon")]
struct Cli {
    /// Path to the output socket
    #[arg(long, default_value = "/run/quick.sock")]
    socket: PathBuf,

    /// Path to the input socket to listen for PCM audio
    #[arg(long)]
    listen: PathBuf,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    log_level: LogLevel,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(cli.log_level))
        .init();
    heard::run(cli.socket, cli.listen).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_info_log_level() {
        let cli = Cli::try_parse_from(["heard", "--listen", "in.sock"]).unwrap();
        assert!(matches!(cli.log_level, LogLevel::Info));
    }

    #[test]
    fn parses_debug_log_level() {
        let cli =
            Cli::try_parse_from(["heard", "--listen", "in.sock", "--log-level", "debug"]).unwrap();
        assert!(matches!(cli.log_level, LogLevel::Debug));
    }
}
