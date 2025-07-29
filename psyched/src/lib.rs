use anyhow::Result;
use chrono::Utc;
use indexmap::IndexMap;
use psyche::models::{MemoryEntry, Sensation};
use psyche::wit::{link_sources, Wit as PipelineWit};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, info, trace};

pub use config::*;
use uuid::Uuid;

pub mod config;
mod db_memory;
pub mod distillers;
mod file_memory;
pub mod llm_config;
mod memory_client;
pub mod router;
pub mod sensor;
mod wit;

/// Identity information loaded from `soul/identity.toml`.
fn default_name() -> String {
    "Unknown".into()
}

#[derive(Deserialize)]
pub struct Identity {
    #[serde(default = "default_name")]
    pub name: String,
    pub role: Option<String>,
    pub purpose: Option<String>,
    /// Wits available to this identity.
    #[serde(default)]
    pub wit: IndexMap<String, wit::WitConfig>,
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

async fn load_identity(path: &Path) -> Result<Identity> {
    if path.exists() {
        let text = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&text)?)
    } else {
        let mut wit = IndexMap::new();
        wit.insert(
            "combobulator".into(),
            wit::WitConfig {
                input: "sensation/chat".into(),
                output: "instant".into(),
                prompt: "{input}".into(),
                priority: 0,
                beat_mod: 1,
                feedback: None,
                llm: None,
                postprocess: None,
            },
        );
        wit.insert(
            "memory".into(),
            wit::WitConfig {
                input: "instant".into(),
                output: "situation".into(),
                prompt: "{input}".into(),
                priority: 0,
                beat_mod: 4,
                feedback: None,
                llm: None,
                postprocess: Some("flatten_links".into()),
            },
        );
        Ok(Identity {
            name: "Unknown".into(),
            role: None,
            purpose: None,
            wit,
        })
    }
}

struct LoadedPipelineWit {
    #[allow(dead_code)]
    name: String,
    cfg: wit::WitConfig,
    wit: PipelineWit,
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

impl LoadedPipelineWit {
    fn new(
        name: String,
        cfg: wit::WitConfig,
        llm: std::sync::Arc<psyche::llm::LlmInstance>,
    ) -> Self {
        let prompt_template = cfg.prompt.clone();
        let pp: Option<
            fn(&[psyche::models::MemoryEntry], &str) -> anyhow::Result<serde_json::Value>,
        > = match name.as_str() {
            "combobulator" | "memory" => {
                Some(link_sources as fn(&[psyche::models::MemoryEntry], &str) -> _)
            }
            _ => None,
        };
        let default_llm: Box<dyn psyche::llm::CanChat> = Box::new(
            psyche::llm::limited::LimitedChat::new(llm.chat.clone(), llm.semaphore.clone()),
        );
        let wit = PipelineWit {
            config: psyche::wit::WitConfig {
                name: name.clone(),
                input_kind: cfg.input.clone(),
                output_kind: cfg.output.clone(),
                prompt_template,
                post_process: pp,
            },
            llm: default_llm,
            profile: (*llm.profile).clone(),
        };
        Self {
            name,
            cfg,
            wit,
            llm,
            offsets: HashMap::new(),
        }
    }

    async fn collect(&mut self, store: &impl db_memory::QueryMemory) -> Result<Vec<MemoryEntry>> {
        trace!(wit = %self.name, "collecting inputs");
        let mut out = Vec::new();
        let kind = &self.cfg.input;
        let entries = store.query_by_kind(kind).await?;
        let offset = self.offsets.entry(kind.clone()).or_insert(0);
        if *offset < entries.len() {
            out.extend_from_slice(&entries[*offset..]);
            *offset = entries.len();
        }
        if !out.is_empty() {
            debug!(wit = %self.name, count = out.len(), "input entries collected");
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

async fn notify_router(router: &router::Router, entry: &MemoryEntry) {
    if let Some(sock) = router.socket_for(&entry.kind) {
        if let Ok(mut stream) = UnixStream::connect(sock).await {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = stream.write_all(line.as_bytes()).await;
            }
        }
    }
}

async fn fire_and_forget_recall(socket: &Path, query: &str) -> std::io::Result<()> {
    if !socket.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "socket missing",
        ));
    }
    let mut stream = UnixStream::connect(socket).await?;
    use tokio::io::AsyncWriteExt;
    stream.write_all(b"/recall\n").await?;
    stream.write_all(query.as_bytes()).await?;
    stream.write_all(b"\n\n").await?;
    Ok(())
}

/// Runs the psyched daemon until `shutdown` is triggered.
pub async fn run(
    socket: PathBuf,
    soul: PathBuf,
    identity: PathBuf,
    beat_duration: std::time::Duration,
    registry: std::sync::Arc<psyche::llm::LlmRegistry>,
    profile: std::sync::Arc<psyche::llm::LlmProfile>,
    llms: Vec<std::sync::Arc<psyche::llm::LlmInstance>>,
    memory_sock: PathBuf,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), "psyched listening");

    let memory_dir = soul.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await?;

    // load daemon configuration and spawn child processes from the identity file
    let psyche_cfg_path = identity.clone();
    let psyche_cfg = match config::load(&psyche_cfg_path).await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::debug!(error = %e, path = %psyche_cfg_path.display(), "no psyche config");
            config::PsycheConfig::default()
        }
    };
    let mut sensor_children = Vec::new();
    for (name, cfg) in &psyche_cfg.sensor {
        if !cfg.enabled {
            continue;
        }
        match sensor::launch_sensor(name, cfg).await {
            Ok(child) => sensor_children.push(child),
            Err(e) => tracing::warn!(sensor = %name, error = %e, "failed to launch sensor"),
        }
    }
    let wit_cfgs: Vec<_> = psyche_cfg
        .wit
        .iter()
        .map(|(n, c)| {
            let mut cfg = c.clone();
            if cfg.name.is_empty() {
                cfg.name = n.clone();
            }
            cfg
        })
        .collect();
    let mut distillers: Vec<distillers::Distiller> = wit_cfgs
        .iter()
        .cloned()
        .map(distillers::Distiller::new)
        .collect();
    for d in &mut distillers {
        let _ = d.spawn().await;
    }
    let router = router::Router::from_configs(&wit_cfgs);

    let cfg_path = identity;
    let identity = load_identity(&cfg_path).await?;
    debug!(identity = %cfg_path.display(), "loaded identity configuration");
    let mut llm_rr = 0usize;
    let mut pipeline: Vec<LoadedPipelineWit> = identity
        .wit
        .iter()
        .filter(|(_, c)| c.priority == 0)
        .map(|(n, c)| {
            let llm = llms[llm_rr % llms.len()].clone();
            llm_rr += 1;
            LoadedPipelineWit::new(n.clone(), c.clone(), llm)
        })
        .collect();

    debug!(wits = pipeline.len(), "loaded pipeline wits");

    let backend =
        if let (Ok(qurl), Ok(nurl)) = (std::env::var("QDRANT_URL"), std::env::var("NEO4J_URL")) {
            tracing::debug!(qdrant = %qurl, neo4j = %nurl, "connecting to backends");
            match qdrant_client::prelude::QdrantClient::from_url(&qurl).build() {
                Ok(qdrant) => {
                    let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
                    let pass = std::env::var("NEO4J_PASS").unwrap_or_else(|_| "password".into());
                    match neo4rs::Graph::new(&nurl, user, pass) {
                        Ok(graph) => Some(std::sync::Arc::new(psyche::memory::QdrantNeo4j {
                            qdrant,
                            graph,
                        })),
                        Err(e) => {
                            tracing::warn!(error = %e, "neo4j connection failed");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "qdrant connection failed");
                    None
                }
            }
        } else {
            None
        };

    trace!("using memory backend");

    let memory_store = db_memory::DbMemory::new(
        memory_dir.clone(),
        backend,
        &*registry.embed,
        &*profile,
        memory_sock.clone(),
    );
    let mut wit_round_robin = 1usize.min(llms.len());
    let mut wits: Vec<LoadedWit> = identity
        .wit
        .into_iter()
        .filter(|(_, c)| c.priority > 0)
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
                for d in &mut pipeline {
                    if beat_counter % d.cfg.beat_mod == 0 {
                        let input = d.collect(&memory_store).await?;
                        if input.is_empty() { continue; }
                        let mut output = d.wit.distill(input).await?;
                        for entry in &mut output {
                            entry.kind = d.cfg.output.clone();
                            if let Some(ref post) = d.cfg.postprocess {
                                if post == "flatten_links" {
                                    entry.what = flatten_links(&entry.what);
                                }
                            }
                        }
                        memory_store.append_entries(&output).await?;
                        for entry in &output {
                            notify_router(&router, entry).await;
                        }
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
                        let how = psyche::utils::first_sentence(&response);
                        let out_id = memory_store.store(&w.cfg.output, &response).await?;
                        if let Ok(uuid) = Uuid::parse_str(&out_id) {
                            let entry = MemoryEntry {
                                id: uuid,
                                when: Utc::now(),
                                kind: w.cfg.output.clone(),
                                what: json!(response),
                                how: how.clone(),
                            };
                            notify_router(&router, &entry).await;
                        }
                        if w.cfg.postprocess.as_deref() == Some("recall") {
                            if let Err(e) = fire_and_forget_recall(&memory_sock, &how).await {
                                tracing::trace!(error = %e, "recall send failed");
                            }
                        }
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
