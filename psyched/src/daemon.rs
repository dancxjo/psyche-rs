use std::env;
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::info;

/// Spawn the `rememberd` daemon.
pub async fn spawn_rememberd(socket: &Path, memory_dir: &Path) -> anyhow::Result<Child> {
    let exe = env::var("CARGO_BIN_EXE_rememberd").unwrap_or_else(|_| {
        let mut p = env::current_exe().expect("exe");
        p.pop();
        p.pop();
        p.push("rememberd");
        p.to_string_lossy().into_owned()
    });
    let mut cmd = Command::new(exe);
    cmd.arg("--socket")
        .arg(socket)
        .arg("--memory-dir")
        .arg(memory_dir)
        .arg("--daemon");
    info!(daemon = "rememberd", socket = %socket.display(), dir = %memory_dir.display(), "launching daemon");
    Ok(cmd.spawn()?)
}

/// Spawn the `would` daemon if motors are configured.
pub async fn spawn_would(socket: &Path, config: &Path) -> anyhow::Result<Child> {
    let exe = env::var("CARGO_BIN_EXE_would").unwrap_or_else(|_| {
        let mut p = env::current_exe().expect("exe");
        p.pop();
        p.pop();
        p.push("would");
        p.to_string_lossy().into_owned()
    });
    let mut cmd = Command::new(exe);
    cmd.arg("--socket")
        .arg(socket)
        .arg("--config")
        .arg(config)
        .arg("--daemon");
    info!(daemon = "would", socket = %socket.display(), "launching daemon");
    Ok(cmd.spawn()?)
}

use crate::config::SpokenConfig;

/// Spawn the `spoken` daemon if configured.
pub async fn spawn_spoken(cfg: &SpokenConfig) -> anyhow::Result<Child> {
    let exe = env::var("CARGO_BIN_EXE_spoken").unwrap_or_else(|_| {
        let mut p = env::current_exe().expect("exe");
        p.pop();
        p.pop();
        p.push("spoken");
        p.to_string_lossy().into_owned()
    });
    let mut cmd = Command::new(exe);
    cmd.arg("--socket")
        .arg(&cfg.socket)
        .arg("--tts-url")
        .arg(&cfg.tts_url)
        .arg("--speaker-id")
        .arg(&cfg.speaker_id)
        .arg("--language-id")
        .arg(&cfg.language_id)
        .arg("--log-level")
        .arg(&cfg.log_level)
        .arg("--daemon");
    info!(daemon = "spoken", socket = %cfg.socket, speaker = %cfg.speaker_id, "launching daemon");
    Ok(cmd.spawn()?)
}
