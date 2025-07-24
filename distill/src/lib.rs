//! Streaming summarization engine used by the `distill` CLI.
//!
//! The [`run`] function orchestrates reading batched input, rendering a prompt
//! template, and streaming the summary from an Ollama instance.
//!
//! ```no_run
//! use distill::Config;
//! use tokio::io::{stdin, stdout};
//! # async fn example() -> anyhow::Result<()> {
//! let cfg = Config {
//!     continuous: false,
//!     lines: 1,
//!     prompt: "Summarize: {{current}}".into(),
//!     model: "llama3".into(),
//!     terminal: "\n".into(),
//!     history_depth: 1,
//!     beat: 0,
//!     trim_newlines: true,
//! };
//! let ollama = ollama_rs::Ollama::try_new("http://localhost:11434")?;
//! distill::run(cfg, ollama, tokio::io::BufReader::new(stdin()), stdout()).await?;
//! # Ok(()) }
//! ```
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::completion::GenerationResponseStream;
use ollama_rs::models::pull::PullModelStatus;
use ollama_rs::{error::OllamaError, Ollama};
use std::collections::VecDeque;
use tera::{Context, Tera};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::{sleep, Duration};
use tokio_stream::StreamExt;
use tracing::{debug, trace, warn};

/// Configuration for [`run`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Run continuously, using each summary as the next `{{previous}}` value.
    pub continuous: bool,
    /// Number of lines per batch.
    pub lines: usize,
    /// Prompt template with `{{previous}}` and `{{current}}` placeholders.
    pub prompt: String,
    /// Model name for the Ollama API.
    pub model: String,
    /// Delimiter printed after each response.
    ///
    /// The delimiter itself is written without an extra newline, so using
    /// `"\n"` results in a single trailing newline per summary.
    pub terminal: String,
    /// How many prior summaries to include in {{previous}}
    pub history_depth: usize,
    /// Delay between batches in milliseconds
    pub beat: u64,
    /// Trim newline tokens emitted by the LLM
    pub trim_newlines: bool,
}

/// Processes the input stream and writes summaries to the output stream.
pub async fn run<R, W>(cfg: Config, ollama: Ollama, input: R, mut output: W) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(input).lines();
    let mut batch = Vec::new();
    let mut history: VecDeque<String> = VecDeque::new();

    while let Some(line) = reader.next_line().await? {
        if cfg.trim_newlines && line.trim().is_empty() {
            continue;
        }
        batch.push(line);
        if batch.len() >= cfg.lines {
            let current = batch.join("\n");
            batch.clear();
            let previous = if cfg.history_depth == 0 {
                String::new()
            } else {
                history.iter().cloned().collect::<Vec<_>>().join("\n\n")
            };
            let summary = summarize_into(&ollama, &cfg, &previous, &current, &mut output).await?;
            output.write_all(cfg.terminal.as_bytes()).await?;
            if cfg.continuous && cfg.history_depth > 0 {
                history.push_back(summary);
                while history.len() > cfg.history_depth {
                    history.pop_front();
                }
            }
            if cfg.beat > 0 {
                sleep(Duration::from_millis(cfg.beat)).await;
            }
        }
    }

    if !batch.is_empty() {
        let current = batch.join("\n");
        let previous = if cfg.history_depth == 0 {
            String::new()
        } else {
            history.iter().cloned().collect::<Vec<_>>().join("\n\n")
        };
        let summary = summarize_into(&ollama, &cfg, &previous, &current, &mut output).await?;
        output.write_all(cfg.terminal.as_bytes()).await?;
        if cfg.continuous && cfg.history_depth > 0 {
            history.push_back(summary);
            while history.len() > cfg.history_depth {
                history.pop_front();
            }
        }
    }

    output.flush().await?;
    Ok(())
}

async fn summarize_into<W>(
    ollama: &Ollama,
    cfg: &Config,
    previous: &str,
    current: &str,
    output: &mut W,
) -> anyhow::Result<String>
where
    W: AsyncWrite + Unpin,
{
    let mut ctx = Context::new();
    ctx.insert("previous", previous);
    ctx.insert("current", current);
    let rendered = Tera::one_off(&cfg.prompt, &ctx, true)?;
    trace!(prompt = %rendered, "llm prompt");

    let req = GenerationRequest::new(cfg.model.clone(), rendered).system("You summarize text.");

    let mut stream: GenerationResponseStream = match ollama.generate_stream(req.clone()).await {
        Ok(s) => s,
        Err(e) => match e {
            OllamaError::Other(msg) if msg.contains("not found") && msg.contains("pull") => {
                pull_with_progress(ollama, cfg.model.clone()).await?;
                ollama.generate_stream(req).await?
            }
            other => return Err(other.into()),
        },
    };
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(responses) => {
                for resp in responses {
                    let mut text = resp.response;
                    if let Some(t) = text.strip_prefix('\u{FEFF}') {
                        text = t.to_string();
                    }
                    if cfg.trim_newlines && text == "\n" {
                        continue;
                    }
                    output_token(&text);
                    out.push_str(&text);
                }
            }
            Err(_) => break,
        }
    }

    let processed = if cfg.trim_newlines {
        out.lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        out.clone()
    };

    output.write_all(processed.as_bytes()).await?;
    output.flush().await?;

    debug!(response = %processed, "llm response");
    Ok(processed)
}

fn output_token(token: &str) {
    trace!(token, "stream token");
}

async fn pull_with_progress(ollama: &Ollama, model: String) -> anyhow::Result<()> {
    warn!(%model, "pulling missing model");
    let pb = ProgressBar::new_spinner();
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    let mut stream = ollama.pull_model_stream(model, false).await?;
    while let Some(status) = stream.next().await {
        let status = status?;
        trace!(status = ?status, "pull progress");
        match status {
            PullModelStatus {
                message,
                total: Some(t),
                completed: Some(c),
                ..
            } => {
                if pb.length().is_none() {
                    pb.set_style(
                        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} {msg}")
                            .unwrap(),
                    );
                    pb.set_length(t);
                }
                pb.set_message(message);
                pb.set_position(c);
            }
            PullModelStatus { message, .. } => {
                pb.set_message(message);
                pb.tick();
            }
        }
    }
    pb.finish_and_clear();
    Ok(())
}
