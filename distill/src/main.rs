use clap::Parser;
use distill::{run, Config};
use ollama_rs::Ollama;
use tokio::fs::File;
use tokio::io::{stdin, stdout, BufReader};
use tracing_subscriber::EnvFilter;

const CONT_PROMPT: &str = "This was the previous situation: {{previous}}\n\nThen these things happened: {{current}}\n\nMake a one sentence summary that explains the entire story.";
const SIMPLE_PROMPT: &str = "These things happened: {{current}}\n\nMake a one sentence summary that explains the entire story.";

#[derive(Parser, Debug)]
#[command(name = "distill", version)]
struct Cli {
    /// Run continuously, reusing each summary as {{previous}}
    #[arg(long)]
    continuous: bool,

    /// Lines per batch
    #[arg(short = 'n', long, default_value_t = 1)]
    lines: usize,

    /// Prompt template with placeholders
    #[arg(long)]
    prompt: Option<String>,

    /// Input file (stdin if omitted)
    #[arg(long)]
    input: Option<std::path::PathBuf>,

    /// Output file (stdout if omitted)
    #[arg(long)]
    output: Option<std::path::PathBuf>,

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
    };

    let ollama = Ollama::try_new(&cli.llm_url)?;

    let input: Box<dyn tokio::io::AsyncBufRead + Unpin> = match cli.input {
        Some(p) => Box::new(BufReader::new(File::open(p).await?)),
        None => Box::new(BufReader::new(stdin())),
    };

    let output: Box<dyn tokio::io::AsyncWrite + Unpin> = match cli.output {
        Some(p) => Box::new(File::create(p).await?),
        None => Box::new(stdout()),
    };

    run(cfg, ollama, input, output).await
}
