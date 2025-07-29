use crate::config::DistillerConfig;
use std::path::PathBuf;
use tokio::process::{Child, Command};
use tokio::time::{sleep, Duration};
use tracing::{error, info};

/// Process wrapper for a single `distilld` instance.
pub struct Distiller {
    cfg: DistillerConfig,
    child: Option<Child>,
    socket: PathBuf,
}

impl Distiller {
    pub fn new(cfg: DistillerConfig) -> Self {
        let socket = PathBuf::from(format!("/run/psyche/{}.sock", cfg.name));
        Self {
            cfg,
            child: None,
            socket,
        }
    }

    /// Spawn the `distilld` process.
    pub async fn spawn(&mut self) -> anyhow::Result<()> {
        let exe = std::env::var("CARGO_BIN_EXE_distilld").unwrap_or_else(|_| {
            let mut p = std::env::current_exe().expect("exe");
            p.pop();
            p.pop();
            p.push("distilld");
            p.to_string_lossy().into_owned()
        });
        let mut cmd = Command::new(exe);
        if let Some(ref t) = self.cfg.prompt {
            cmd.arg("--prompt").arg(t);
        }
        cmd.arg("--daemon");
        let child = cmd.spawn()?;
        self.child = Some(child);
        info!(distiller = %self.cfg.name, "spawned distilld");
        Ok(())
    }

    /// Check if the process has exited and restart if needed.
    pub async fn monitor(&mut self) -> anyhow::Result<()> {
        if let Some(child) = self.child.as_mut() {
            if let Some(status) = child.try_wait()? {
                error!(distiller = %self.cfg.name, ?status, "distilld exited");
                sleep(Duration::from_secs(1)).await;
                self.spawn().await?;
            }
        }
        Ok(())
    }
}
