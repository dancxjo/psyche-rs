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
//! };
//! let ollama = ollama_rs::Ollama::try_new("http://localhost:11434")?;
//! distill::run(cfg, ollama, tokio::io::BufReader::new(stdin()), stdout()).await?;
//! # Ok(()) }
//! ```
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::{ChatMessage, MessageRole};
use ollama_rs::{error::OllamaError, Ollama};
use tera::{Context, Tera};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_stream::StreamExt;
use tracing::{debug, trace};

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
}

/// Processes the input stream and writes summaries to the output stream.
pub async fn run<R, W>(cfg: Config, ollama: Ollama, input: R, mut output: W) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(input).lines();
    let mut batch = Vec::new();
    let mut previous = String::new();

    while let Some(line) = reader.next_line().await? {
        batch.push(line);
        if batch.len() >= cfg.lines {
            let current = batch.join("\n");
            batch.clear();
            let summary = summarize(&ollama, &cfg, &previous, &current).await?;
            output.write_all(summary.as_bytes()).await?;
            output.write_all(b"\n").await?;
            if cfg.continuous {
                previous = summary;
            }
        }
    }

    if !batch.is_empty() {
        let current = batch.join("\n");
        let summary = summarize(&ollama, &cfg, &previous, &current).await?;
        output.write_all(summary.as_bytes()).await?;
        output.write_all(b"\n").await?;
    }

    output.flush().await?;
    Ok(())
}

async fn summarize(
    ollama: &Ollama,
    cfg: &Config,
    previous: &str,
    current: &str,
) -> anyhow::Result<String> {
    let mut ctx = Context::new();
    ctx.insert("previous", previous);
    ctx.insert("current", current);
    let rendered = Tera::one_off(&cfg.prompt, &ctx, true)?;
    trace!(prompt = %rendered, "llm prompt");

    let req = ChatMessageRequest::new(
        cfg.model.clone(),
        vec![
            ChatMessage::new(MessageRole::System, "You summarize text.".into()),
            ChatMessage::new(MessageRole::User, rendered),
        ],
    );

    let mut stream = match ollama.send_chat_messages_stream(req.clone()).await {
        Ok(s) => s,
        Err(e) => match e {
            OllamaError::Other(msg) if msg.contains("not found") && msg.contains("pull") => {
                ollama.pull_model(cfg.model.clone(), false).await?;
                ollama.send_chat_messages_stream(req).await?
            }
            other => return Err(other.into()),
        },
    };
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(resp) => {
                let mut text = resp.message.content;
                if let Some(t) = text.strip_prefix('\u{FEFF}') {
                    text = t.to_string();
                }
                output_token(&text);
                out.push_str(&text);
            }
            Err(_) => break,
        }
    }
    debug!(response = %out, "llm response");
    Ok(out)
}

fn output_token(token: &str) {
    trace!(token, "stream token");
}
