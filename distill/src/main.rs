use clap::Parser;
use distill::{run, Config};
use ollama_rs::Ollama;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{stdin, stdout, BufReader};
use tracing_subscriber::EnvFilter;

const CONT_PROMPT: &str = "This was the previous situation: {{previous}}\n\nThen these things happened: {{current}}\n\nMake a one sentence summary that explains the entire story.";
const SIMPLE_PROMPT: &str = "These things happened: {{current}}\n\nMake a one sentence summary that explains the entire story.";

#[derive(Parser, Debug)]
#[command(name = "distill", version)]
struct Cli {
    /// Run continuously, reusing each summary as {{previous}}
    #[arg(short = 'c', long)]
    continuous: bool,

    /// Lines per batch
    #[arg(short = 'n', long, default_value_t = 1)]
    lines: usize,

    /// Prompt template with placeholders
    #[arg(short = 'p', long)]
    prompt: Option<String>,

    /// Input path or - for stdin
    #[arg(short = 'i', long, default_value = "-")]
    input: std::path::PathBuf,

    /// Output path or stdout
    #[arg(short = 'o', long)]
    output: Option<std::path::PathBuf>,

    /// Number of previous summaries to include in {{previous}}
    #[arg(short = 'd', long, default_value_t = 1)]
    history_depth: usize,

    /// Milliseconds to wait between batches
    #[arg(short = 'b', long, default_value_t = 0)]
    beat: u64,

    /// Base URL for Ollama
    #[arg(long, default_value = "http://localhost:11434")]
    llm_url: String,

    /// Model name
    #[arg(long, default_value = "gemma3n")]
    model: String,

    /// Delimiter printed after each response
    #[arg(long, default_value = "\n")]
    terminal: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let prompt = cli.prompt.unwrap_or_else(|| {
        if cli.continuous {
            CONT_PROMPT
        } else {
            SIMPLE_PROMPT
        }
        .to_string()
    });

    let cfg = Config {
        continuous: cli.continuous,
        lines: cli.lines,
        prompt,
        model: cli.model.clone(),
        terminal: cli.terminal.clone(),
        history_depth: cli.history_depth,
        beat: cli.beat,
    };

    let ollama = Ollama::try_new(&cli.llm_url)?;

    let input: Box<dyn tokio::io::AsyncBufRead + Unpin> = if cli.input == PathBuf::from("-") {
        Box::new(BufReader::new(stdin()))
    } else {
        Box::new(BufReader::new(File::open(cli.input).await?))
    };

    let output: Box<dyn tokio::io::AsyncWrite + Unpin> = match cli.output {
        Some(p) => Box::new(File::create(p).await?),
        None => Box::new(stdout()),
    };

    run(cfg, ollama, input, output).await
}
