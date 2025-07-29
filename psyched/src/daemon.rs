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
