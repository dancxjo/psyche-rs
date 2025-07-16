use anyhow::Result;
use chrono::Utc;
use psyche::distiller::{Combobulator, Distiller, Passthrough};
use psyche::models::{MemoryEntry, Sensation};
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use uuid::Uuid;

async fn handle_stream(stream: UnixStream, dir: PathBuf) -> Result<()> {
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
    let path = dir.join("sensation.jsonl");
    append(&path, &sensation).await?;
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
struct DistillerConfig {
    input_kinds: Vec<String>,
    output_kind: String,
    prompt_template: Option<String>,
    beat_mod: usize,
    #[serde(default)]
    postprocess: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    distiller: HashMap<String, DistillerConfig>,
}

struct LoadedDistiller {
    name: String,
    cfg: DistillerConfig,
    distiller: Box<dyn Distiller + Send>,
    offsets: HashMap<String, usize>,
}

impl LoadedDistiller {
    fn new(name: String, cfg: DistillerConfig) -> Self {
        let distiller: Box<dyn Distiller + Send> = match name.as_str() {
            "memory" => Box::new(Passthrough),
            _ => Box::new(Combobulator),
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
    memory_dir: PathBuf,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;

    let cfg_path = memory_dir.join("psyche.toml");
    let cfg: Config = if cfg_path.exists() {
        let text = tokio::fs::read_to_string(&cfg_path).await?;
        toml::from_str(&text)?
    } else {
        let mut map = HashMap::new();
        map.insert(
            "combobulator".into(),
            DistillerConfig {
                input_kinds: vec!["sensation/chat".into()],
                output_kind: "instant".into(),
                prompt_template: None,
                beat_mod: 1,
                postprocess: None,
            },
        );
        Config { distiller: map }
    };

    let mut distillers: Vec<LoadedDistiller> = cfg
        .distiller
        .into_iter()
        .map(|(n, c)| LoadedDistiller::new(n, c))
        .collect();

    let mut beat = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut beat_counter = 0usize;

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Ok((stream, _)) = listener.accept() => {
                handle_stream(stream, memory_dir.clone()).await?;
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
            }
        }
    }
    Ok(())
}
