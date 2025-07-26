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
        let mut cmd = Command::new("distilld");
        cmd.arg("--input-kind").arg(&self.cfg.input_kind);
        cmd.arg("--output-kind").arg(&self.cfg.output_kind);
        if let Some(ref t) = self.cfg.prompt_template {
            cmd.arg("--prompt-template").arg(t);
        }
        if let Some(ref c) = self.cfg.config {
            cmd.arg("--config").arg(c);
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
