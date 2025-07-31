use clap::{Args, Parser, Subcommand};
use daemon_common::{LogLevel, maybe_daemonize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "whisperd",
    about = "Audio ingestion and transcription daemon",
    long_about = "Audio ingestion and transcription daemon.\n\nInput Protocol:\n  The stream may optionally begin with a line of the form:\n    @{2025-07-31T14:00:00-07:00}\n  This sets the timestamp used for the input data. If omitted or invalid, the system defaults to the current local time."
)]
struct Cli {
    #[command(flatten)]
    run: RunArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Path to the Unix socket used for audio input and transcript output
    #[arg(long, default_value = "/run/psyched/ear.sock")]
    socket: PathBuf,

    /// Path to whisper model
    #[arg(long, env = "WHISPER_MODEL")]
    whisper_model: Option<PathBuf>,

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

#[derive(Subcommand, Debug)]
enum Command {
    /// Output a systemd unit file to stdout
    GenSystemd,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(Command::GenSystemd) = cli.command {
        print!("{}", whisperd::systemd_unit());
        return Ok(());
    }

    let run = cli.run;
    let model = run
        .whisper_model
        .ok_or_else(|| anyhow::anyhow!("--whisper-model is required"))?;
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(run.log_level))
        .init();
    maybe_daemonize(run.daemon)?;
    whisperd::run(run.socket, model, run.silence_ms, run.timeout_ms).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_info_log_level() {
        let cli = Cli::try_parse_from(["whisperd", "--whisper-model", "model.bin"]).unwrap();
        assert!(matches!(cli.run.log_level, LogLevel::Info));
        assert_eq!(cli.run.silence_ms, 1000);
        assert_eq!(cli.run.timeout_ms, 10000);
        assert!(cli.command.is_none());
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
        assert!(matches!(cli.run.log_level, LogLevel::Debug));
        assert_eq!(cli.run.silence_ms, 1500);
        assert_eq!(cli.run.timeout_ms, 5000);
    }

    #[test]
    fn parses_gen_systemd_command() {
        let cli = Cli::try_parse_from(["whisperd", "gen-systemd"]).unwrap();
        assert!(matches!(cli.command, Some(Command::GenSystemd)));
    }
}
