use anyhow::Result;
use indexmap::IndexMap;
use psyche::models::Sensation;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, info, trace};
use would::WouldConfig;

pub use config::*;
use uuid::Uuid;

pub mod config;
pub mod daemon;
mod memory_client;
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

/// Runs the psyched daemon until `shutdown` is triggered.
pub async fn run(
    socket: PathBuf,
    soul: PathBuf,
    identity: PathBuf,
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
    let memory = memory_client::MemoryClient::new(memory_sock.clone());
    let server = tokio::task::spawn_local(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => match read_sensation(stream).await {
                    Ok(s) => {
                        let kind = format!("sensation{}", s.path);
                        if let Err(e) = memory
                            .memorize(&kind, serde_json::to_value(&s).unwrap())
                            .await
                        {
                            tracing::error!(error = %e, "failed to store sensation");
                        } else {
                            debug!(id = %s.id, "sensation stored");
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
    });

    shutdown.await;
    server.abort();
    let _ = server.await;
    for mut child in sensor_children {
        let _ = child.kill().await;
    }
    for mut child in daemon_children {
        let _ = child.kill().await;
    }
    Ok(())
}
