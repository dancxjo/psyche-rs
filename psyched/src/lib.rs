use anyhow::Result;
use chrono::Utc;
use indexmap::IndexMap;
use psyche::distiller::{Distiller, DistillerConfig};
use psyche::models::{MemoryEntry, Sensation};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, info, trace};
use uuid::Uuid;

mod db_memory;
mod file_memory;
pub mod llm_config;
mod wit;

/// Identity information loaded from `soul/identity.toml`.
#[derive(Deserialize)]
pub struct Identity {
    pub name: String,
    pub role: Option<String>,
    pub purpose: Option<String>,
}

/// Read a sensation from a Unix stream until a line containing `---` is found.
async fn read_sensation(stream: UnixStream) -> Result<Sensation> {
    let mut reader = BufReader::new(stream);
    let mut path = String::new();
    reader.read_line(&mut path).await?;
    trace!(path = %path.trim(), "reading sensation path");
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
    debug!(len = text.len(), "sensation text received");
    Ok(Sensation {
        id: Uuid::new_v4().to_string(),
        path: path.trim().to_string(),
        text,
    })
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
    distiller: IndexMap<String, PipelineDistillerConfig>,
    #[serde(default)]
    wit: IndexMap<String, wit::WitConfig>,
}

async fn load_config(path: &Path) -> Result<Config> {
    if path.exists() {
        let text = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&text)?)
    } else {
        let mut map = IndexMap::new();
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
            wit: IndexMap::new(),
        })
    }
}

struct LoadedDistiller {
    #[allow(dead_code)]
    name: String,
    cfg: PipelineDistillerConfig,
    distiller: Distiller,
    llm: std::sync::Arc<psyche::llm::LlmInstance>,
    offsets: HashMap<String, usize>,
}

struct LoadedWit {
    name: String,
    cfg: wit::WitConfig,
    beat: usize,
    interval: usize,
    llm: std::sync::Arc<psyche::llm::LlmInstance>,
}

fn priority_to_interval(priority: usize) -> usize {
    if priority == 0 {
        return 1;
    }
    fn is_prime(n: usize) -> bool {
        if n < 2 {
            return false;
        }
        for i in 2..=((n as f64).sqrt() as usize) {
            if n % i == 0 {
                return false;
            }
        }
        true
    }

    let mut count = 0;
    let mut num = 2;
    loop {
        if is_prime(num) {
            count += 1;
            if count == priority {
                return num;
            }
        }
        num += 1;
    }
}

impl LoadedWit {
    fn new(
        name: String,
        cfg: wit::WitConfig,
        llm: std::sync::Arc<psyche::llm::LlmInstance>,
    ) -> Self {
        let interval = priority_to_interval(cfg.priority);
        Self {
            name,
            cfg,
            beat: 0,
            interval,
            llm,
        }
    }
}

impl LoadedDistiller {
    fn new(
        name: String,
        cfg: PipelineDistillerConfig,
        llm: std::sync::Arc<psyche::llm::LlmInstance>,
    ) -> Self {
        let prompt_template = cfg
            .prompt_template
            .clone()
            .unwrap_or_else(|| "{input}".to_string());
        let pp: Option<
            fn(&[psyche::models::MemoryEntry], &str) -> anyhow::Result<serde_json::Value>,
        > = match name.as_str() {
            "combobulator" | "memory" => Some(
                psyche::distiller::link_sources as fn(&[psyche::models::MemoryEntry], &str) -> _,
            ),
            _ => None,
        };
        let default_llm: Box<dyn psyche::llm::CanChat> = Box::new(
            psyche::llm::limited::LimitedChat::new(llm.chat.clone(), llm.semaphore.clone()),
        );
        let distiller = Distiller {
            config: DistillerConfig {
                name: name.clone(),
                input_kind: cfg.input_kinds.first().cloned().unwrap_or_default(),
                output_kind: cfg.output_kind.clone(),
                prompt_template,
                post_process: pp,
            },
            llm: default_llm,
            profile: (*llm.profile).clone(),
        };
        Self {
            name,
            cfg,
            distiller,
            llm,
            offsets: HashMap::new(),
        }
    }

    async fn collect(&mut self, dir: &Path) -> Result<Vec<MemoryEntry>> {
        trace!(distiller = %self.name, "collecting inputs");
        let mut out = Vec::new();
        for kind in &self.cfg.input_kinds {
            let path = dir.join(format!("{}.jsonl", kind.split('/').next().unwrap()));
            let offset = self.offsets.entry(kind.clone()).or_insert(0);
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            let lines: Vec<_> = content.lines().collect();
            if *offset < lines.len() {
                for line in &lines[*offset..] {
                    if kind.starts_with("sensation") {
                        let s: Sensation = serde_json::from_str(line)?;
                        let entry_kind = format!("sensation{}", s.path);
                        if entry_kind.starts_with(kind) {
                            out.push(MemoryEntry {
                                id: Uuid::parse_str(&s.id)?,
                                kind: entry_kind,
                                when: Utc::now(),
                                what: json!(s.text),
                                how: String::new(),
                            });
                        }
                    } else {
                        let mut e: MemoryEntry = serde_json::from_str(line)?;
                        e.kind = kind.clone();
                        out.push(e);
                    }
                }
                *offset = lines.len();
            }
        }
        debug!(distiller = %self.name, count = out.len(), "input entries collected");
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
    llms: Vec<std::sync::Arc<psyche::llm::LlmInstance>>,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), "psyched listening");

    let memory_dir = soul.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await?;

    let cfg_path = pipeline;
    let cfg = load_config(&cfg_path).await?;

    let mut llm_rr = 0usize;
    let mut distillers: Vec<LoadedDistiller> = cfg
        .distiller
        .into_iter()
        .map(|(n, c)| {
            let llm = llms[llm_rr % llms.len()].clone();
            llm_rr += 1;
            LoadedDistiller::new(n, c, llm)
        })
        .collect();

    let backend =
        if let (Ok(qurl), Ok(nurl)) = (std::env::var("QDRANT_URL"), std::env::var("NEO4J_URL")) {
            let qdrant = qdrant_client::prelude::QdrantClient::from_url(&qurl).build();
            let qdrant = qdrant.expect("qdrant");
            let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
            let pass = std::env::var("NEO4J_PASS").unwrap_or_else(|_| "password".into());
            let graph = neo4rs::Graph::new(&nurl, user, pass)?;
            Some(std::sync::Arc::new(psyche::memory::QdrantNeo4j {
                qdrant,
                graph,
            }))
        } else {
            None
        };

    let memory_store =
        db_memory::DbMemory::new(memory_dir.clone(), backend, &*registry.embed, &*profile);
    let mut wit_round_robin = 1usize.min(llms.len());
    let mut wits: Vec<LoadedWit> = cfg
        .wit
        .into_iter()
        .enumerate()
        .map(|(i, (n, c))| {
            let llm = if let Some(ref name) = c.llm {
                llms.iter()
                    .find(|l| l.name == *name)
                    .cloned()
                    .unwrap_or_else(|| llms.get(0).cloned().expect("llms vector is empty"))
            } else if i == 0 {
                llms.get(0).cloned().expect("llms vector is empty")
            } else {
                let l = llms[wit_round_robin % llms.len()].clone();
                wit_round_robin += 1;
                l
            };
            LoadedWit::new(n, c, llm)
        })
        .collect();
    let wit_inputs: std::collections::HashMap<String, String> = wits
        .iter()
        .map(|w| (w.name.clone(), w.cfg.input.clone()))
        .collect();

    let mut beat = tokio::time::interval(beat_duration);
    let mut beat_counter = 0usize;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Sensation>();

    let server = {
        let tx = tx.clone();
        tokio::task::spawn_local(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => match read_sensation(stream).await {
                        Ok(s) => {
                            if tx.send(s).is_err() {
                                break;
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "failed to read sensation"),
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "accept failed");
                        break;
                    }
                }
            }
        })
    };

    tokio::pin!(shutdown);
    let mut pending: VecDeque<Sensation> = VecDeque::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Some(sensation) = rx.recv() => {
                debug!(path = %sensation.path, "received sensation");
                pending.push_back(sensation);
            }
            _ = beat.tick() => {
                beat_counter += 1;
                trace!(beat = beat_counter, "beat tick");
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
                        memory_store.append_entries(&output).await?;
                    }
                }
                for w in &mut wits {
                    w.beat += 1;
                    if w.beat % w.interval == 0 {
                        let items = memory_store.query_latest(&w.cfg.input).await;
                        if items.is_empty() { continue; }
                        let user_prompt = items.join("\n");
                        let system_prompt = w.cfg.prompt.clone();
                        let permit = w.llm.semaphore.clone().acquire_owned().await?;
                        let mut stream = w
                            .llm
                            .chat
                            .chat_stream(&w.llm.profile, &system_prompt, &user_prompt)
                            .await
                            .expect("Failed to get chat stream");
                        use tokio_stream::StreamExt;
                        let mut response = String::new();
                        while let Some(token) = stream.next().await {
                            response.push_str(&token);
                        }
                        drop(permit);
                        let out_id = memory_store.store(&w.cfg.output, &response).await?;
                        if let Some(target) = &w.cfg.feedback {
                            if let Some(kind) = wit_inputs.get(target) {
                                let fb_id = memory_store.store(kind, &response).await?;
                                memory_store.link_summary(&fb_id, &out_id).await?;
                            } else {
                                tracing::warn!(source = %w.name, target = %target, "feedback target missing");
                            }
                        }
                        tracing::info!(wit = %w.name, output = %w.cfg.output, "wit stored output");
                    }
                }
            }
        }
        while let Some(s) = pending.pop_front() {
            if let Err(e) = memory_store.store_sensation(&s).await {
                tracing::error!(error = %e, "failed to store sensation");
            } else {
                debug!(id = %s.id, "sensation stored");
            }
        }
    }
    server.abort();
    let _ = server.await;
    Ok(())
}
