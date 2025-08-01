use clap::{Args, Parser, Subcommand};
use daemon_common::{LogLevel, maybe_daemonize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "whisperd",
    about = "Audio ingestion and transcription daemon",
    long_about = "Audio ingestion and transcription daemon. Timestamps begin when the first segment arrives."
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

    /// Use GPU acceleration when available (pass --no-gpu to disable)
    #[arg(long = "no-gpu", action = clap::ArgAction::SetFalse, default_value_t = true)]
    use_gpu: bool,

    /// Logging verbosity level
    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    /// Milliseconds of silence before transcribing
    #[arg(long, default_value = "1000")]
    silence_ms: u64,

    /// Maximum milliseconds for a single segment
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,

    /// Maximum queued segments awaiting transcription
    #[arg(long, default_value = "16")]
    max_queue: usize,

    /// Run as a background daemon
    #[arg(short = 'd', long)]
    daemon: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Output a systemd unit file to stdout
    GenSystemd,
    /// Download a whisper model
    FetchModel(FetchArgs),
}

#[derive(Args, Debug)]
struct FetchArgs {
    /// Directory to save the model
    #[arg(long, default_value = "/opt/whisper")]
    dir: PathBuf,

    /// Whisper model name
    #[arg(long)]
    model: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::GenSystemd) => {
            print!("{}", whisperd::systemd_unit());
            return Ok(());
        }
        Some(Command::FetchModel(args)) => {
            let model = match args.model {
                Some(m) => m,
                None => whisperd::model::prompt_model()?,
            };
            let path = whisperd::model::download(&model, &args.dir).await?;
            println!("{}", path.display());
            return Ok(());
        }
        None => {}
    }

    let run = cli.run;
    let model = run
        .whisper_model
        .ok_or_else(|| anyhow::anyhow!("--whisper-model is required"))?;
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::from(run.log_level))
        .init();
    maybe_daemonize(run.daemon)?;
    whisperd::run(
        run.socket,
        model,
        run.silence_ms,
        run.timeout_ms,
        run.max_queue,
        run.use_gpu,
    )
    .await
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
        assert_eq!(cli.run.max_queue, 16);
        assert!(cli.run.use_gpu);
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
            "--max-queue",
            "8",
        ])
        .unwrap();
        assert!(matches!(cli.run.log_level, LogLevel::Debug));
        assert_eq!(cli.run.silence_ms, 1500);
        assert_eq!(cli.run.timeout_ms, 5000);
        assert_eq!(cli.run.max_queue, 8);
        assert!(cli.run.use_gpu);
    }

    #[test]
    fn parses_gen_systemd_command() {
        let cli = Cli::try_parse_from(["whisperd", "gen-systemd"]).unwrap();
        assert!(matches!(cli.command, Some(Command::GenSystemd)));
    }

    #[test]
    fn parses_fetch_model_command() {
        let cli = Cli::try_parse_from(["whisperd", "fetch-model"]).unwrap();
        assert!(matches!(cli.command, Some(Command::FetchModel(_))));
    }

    #[test]
    fn parses_no_gpu_flag() {
        let cli =
            Cli::try_parse_from(["whisperd", "--whisper-model", "model.bin", "--no-gpu"]).unwrap();
        assert!(!cli.run.use_gpu);
    }
}
