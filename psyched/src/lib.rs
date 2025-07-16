use anyhow::Result;
use chrono::Utc;
use psyche::distiller::{Distiller, DistillerConfig};
use psyche::models::{MemoryEntry, Sensation};
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use uuid::Uuid;

mod file_memory;
mod wit;

/// Identity information loaded from `soul/identity.toml`.
#[derive(Deserialize)]
pub struct Identity {
    pub name: String,
    pub role: Option<String>,
    pub purpose: Option<String>,
}

async fn handle_stream(stream: UnixStream, sensation_path: PathBuf) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut path = String::new();
    reader.read_line(&mut path).await?;
    let mut text = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim_end() == "---" {
            break;
        }
        text.push_str(&line);
    }
    let text = text.trim_end_matches('\n').to_string();
    let sensation = Sensation {
        id: Uuid::new_v4().to_string(),
        path: path.trim().to_string(),
        text,
    };
    append(&sensation_path, &sensation).await?;
    Ok(())
}

async fn append<T: Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    let line = serde_json::to_string(value)?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    Ok(())
}

async fn append_all<T: Serialize>(path: &PathBuf, values: &[T]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    for v in values {
        let line = serde_json::to_string(v)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    Ok(())
}

#[derive(Deserialize, Clone)]
struct PipelineDistillerConfig {
    input_kinds: Vec<String>,
    output_kind: String,
    #[allow(dead_code)]
    prompt_template: Option<String>,
    beat_mod: usize,
    #[serde(default)]
    postprocess: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    distiller: HashMap<String, PipelineDistillerConfig>,
    #[serde(default)]
    wit: HashMap<String, wit::WitConfig>,
}

async fn load_config(path: &Path) -> Result<Config> {
    if path.exists() {
        let text = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&text)?)
    } else {
        let mut map = HashMap::new();
        map.insert(
            "combobulator".into(),
            PipelineDistillerConfig {
                input_kinds: vec!["sensation/chat".into()],
                output_kind: "instant".into(),
                prompt_template: None,
                beat_mod: 1,
                postprocess: None,
            },
        );
        map.insert(
            "memory".into(),
            PipelineDistillerConfig {
                input_kinds: vec!["instant".into()],
                output_kind: "situation".into(),
                prompt_template: None,
                beat_mod: 4,
                postprocess: Some("flatten_links".into()),
            },
        );
        Ok(Config {
            distiller: map,
            wit: HashMap::new(),
        })
    }
}

struct LoadedDistiller {
    #[allow(dead_code)]
    name: String,
    cfg: PipelineDistillerConfig,
    distiller: Distiller,
    offsets: HashMap<String, usize>,
}

struct LoadedWit {
    name: String,
    cfg: wit::WitConfig,
    beat: usize,
}

impl LoadedWit {
    fn new(name: String, cfg: wit::WitConfig) -> Self {
        Self { name, cfg, beat: 0 }
    }
}

impl LoadedDistiller {
    fn new(name: String, cfg: PipelineDistillerConfig) -> Self {
        let prompt_template = cfg
            .prompt_template
            .clone()
            .unwrap_or_else(|| "{input}".to_string());
        let distiller = Distiller {
            config: DistillerConfig {
                name: name.clone(),
                input_kind: cfg.input_kinds.first().cloned().unwrap_or_default(),
                output_kind: cfg.output_kind.clone(),
                prompt_template,
                post_process: None,
            },
            llm: Box::new(psyche::llm::mock_chat::MockChat::default()),
        };
        Self {
            name,
            cfg,
            distiller,
            offsets: HashMap::new(),
        }
    }

    async fn collect(&mut self, dir: &Path) -> Result<Vec<MemoryEntry>> {
        let mut out = Vec::new();
        for kind in &self.cfg.input_kinds {
            let path = dir.join(format!("{}.jsonl", kind.split('/').next().unwrap()));
            let offset = self.offsets.entry(kind.clone()).or_insert(0);
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            let lines: Vec<_> = content.lines().collect();
            if *offset < lines.len() {
                for line in &lines[*offset..] {
                    if kind.starts_with("sensation/") {
                        let s: Sensation = serde_json::from_str(line)?;
                        out.push(MemoryEntry {
                            id: Uuid::parse_str(&s.id)?,
                            kind: kind.clone(),
                            when: Utc::now(),
                            what: json!(s.text),
                            how: String::new(),
                        });
                    } else {
                        let mut e: MemoryEntry = serde_json::from_str(line)?;
                        e.kind = kind.clone();
                        out.push(e);
                    }
                }
                *offset = lines.len();
            }
        }
        Ok(out)
    }
}

fn flatten_links(val: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match val {
        Value::Array(arr) => {
            let mut flat = Vec::new();
            for v in arr {
                match v {
                    Value::Array(inner) => flat.extend(inner.clone()),
                    _ => flat.push(v.clone()),
                }
            }
            Value::Array(flat)
        }
        _ => val.clone(),
    }
}

/// Runs the psyched daemon until `shutdown` is triggered.
pub async fn run(
    socket: PathBuf,
    soul: PathBuf,
    pipeline: PathBuf,
    beat_duration: std::time::Duration,
    registry: std::sync::Arc<psyche::llm::LlmRegistry>,
    profile: std::sync::Arc<psyche::llm::LlmProfile>,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;

    let memory_dir = soul.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await?;
    let sensation_path = memory_dir.join("sensation.jsonl");

    let cfg_path = pipeline;
    let cfg = load_config(&cfg_path).await?;

    let mut distillers: Vec<LoadedDistiller> = cfg
        .distiller
        .into_iter()
        .map(|(n, c)| LoadedDistiller::new(n, c))
        .collect();

    let memory_store = file_memory::FileMemory::new(memory_dir.clone());
    let mut wits: Vec<LoadedWit> = cfg
        .wit
        .into_iter()
        .map(|(n, c)| LoadedWit::new(n, c))
        .collect();

    let mut beat = tokio::time::interval(beat_duration);
    let mut beat_counter = 0usize;

    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Ok((stream, _)) = listener.accept() => {
                handle_stream(stream, sensation_path.clone()).await?;
            }
            _ = beat.tick() => {
                beat_counter += 1;
                for d in &mut distillers {
                    if beat_counter % d.cfg.beat_mod == 0 {
                        let input = d.collect(&memory_dir).await?;
                        if input.is_empty() { continue; }
                        let mut output = d.distiller.distill(input).await?;
                        for entry in &mut output {
                            entry.kind = d.cfg.output_kind.clone();
                            if let Some(ref post) = d.cfg.postprocess {
                                if post == "flatten_links" {
                                    entry.what = flatten_links(&entry.what);
                                }
                            }
                        }
                        let path = memory_dir.join(format!("{}.jsonl", d.cfg.output_kind));
                        append_all(&path, &output).await?;
                    }
                }
                for w in &mut wits {
                    w.beat += 1;
                    if w.beat % w.cfg.every == 0 {
                        let items = memory_store.query_latest(&w.cfg.input).await;
                        if items.is_empty() { continue; }
                        let user_prompt = items.join("\n");
                        let system_prompt = w.cfg.prompt.clone();
                        let mut stream = registry
                            .chat
                            .chat_stream(&profile, &system_prompt, &user_prompt)
                            .await
                            .expect("Failed to get chat stream");
                        use tokio_stream::StreamExt;
                        let mut response = String::new();
                        while let Some(token) = stream.next().await {
                            response.push_str(&token);
                        }
                        memory_store
                            .store(&w.cfg.output, &response)
                            .await?;
                        tracing::info!(wit = %w.name, output = %w.cfg.output, "wit stored output");
                    }
                }
            }
        }
    }
    Ok(())
}
