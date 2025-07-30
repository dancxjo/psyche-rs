use anyhow::Result;
use chrono::Utc;
use indexmap::IndexMap;
use psyche::models::{MemoryEntry, Sensation};

use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, info, trace};
use would::WouldConfig;

pub use config::*;
use uuid::Uuid;

pub mod config;
pub mod daemon;
mod db_memory;
pub mod distillers;
mod file_memory;
pub mod llm_config;
mod memory_client;
pub mod router;
pub mod sensor;
mod socket_pipe;
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
                input: Some("sensation/chat".into()),
                output: Some("instant".into()),
                prompt: "{input}".into(),
                priority: 0,
                beat_mod: None,
                feedback: None,
                llm: None,
                postprocess: None,
            },
        );
        wit.insert(
            "memory".into(),
            wit::WitConfig {
                input: Some("instant".into()),
                output: Some("situation".into()),
                prompt: "{input}".into(),
                priority: 0,
                beat_mod: None,
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
    registry: std::sync::Arc<psyche::llm::LlmRegistry>,
    profile: std::sync::Arc<psyche::llm::LlmProfile>,
    memory_sock: PathBuf,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), "psyched listening");

    let memory_dir = soul.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await?;

    // spawn core daemons
    let mut daemon_children = Vec::new();
    if let Ok(child) = daemon::spawn_rememberd(&memory_sock, &memory_dir).await {
        daemon_children.push(child);
    }

    // load daemon configuration and spawn child processes from the identity file
    let psyche_cfg_path = identity.clone();
    let psyche_cfg = match config::load(&psyche_cfg_path).await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::debug!(error = %e, path = %psyche_cfg_path.display(), "no psyche config");
            config::PsycheConfig::default()
        }
    };
    if let Some(spk) = psyche_cfg.spoken.clone() {
        if let Ok(child) = daemon::spawn_spoken(&spk).await {
            daemon_children.push(child);
        }
    }
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

    if let Ok(wcfg) = would::WouldConfig::load(&psyche_cfg_path).await {
        if !wcfg.motors.is_empty() {
            if let Ok(child) =
                daemon::spawn_would(Path::new("/run/would.sock"), &psyche_cfg_path).await
            {
                daemon_children.push(child);
            }
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
    let _identity = load_identity(&cfg_path).await?;
    debug!(identity = %cfg_path.display(), "loaded identity configuration");

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

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Sensation>();

    let mut watchers = Vec::new();
    for cfg in psyche_cfg.pipe.values() {
        let sock = PathBuf::from(&cfg.socket);
        let dest = cfg.path.clone();
        let tx_clone = tx.clone();
        let mut deps = Vec::new();
        for dep in &cfg.depends_on {
            if let Some(scfg) = psyche_cfg.sensor.get(dep) {
                let dep_sock = scfg.socket.clone().unwrap_or_else(|| format!("{dep}.sock"));
                deps.push(PathBuf::from(dep_sock));
            }
        }
        watchers.push(tokio::task::spawn_local(
            socket_pipe::watch_socket_when_ready(sock, dest, tx_clone, deps),
        ));
    }

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
        }
        while let Some(s) = pending.pop_front() {
            if let Err(e) = memory_store.store_sensation(&s).await {
                tracing::error!(error = %e, "failed to store sensation");
            } else {
                debug!(id = %s.id, "sensation stored");
            }
        }
        for d in &mut distillers {
            let _ = d.monitor().await;
        }
    }
    server.abort();
    let _ = server.await;
    for w in watchers {
        w.abort();
        let _ = w.await;
    }
    for mut child in sensor_children {
        let _ = child.kill().await;
    }
    for mut child in daemon_children {
        let _ = child.kill().await;
    }
    Ok(())
}
