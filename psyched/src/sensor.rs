use crate::config::SensorConfig;
use tokio::process::{Child, Command};
use tracing::info;

/// Launch a sensor daemon and return the child handle.
pub async fn launch_sensor(name: &str, cfg: &SensorConfig) -> anyhow::Result<Child> {
    let socket = cfg.socket.clone().unwrap_or_else(|| format!("{name}.sock"));
    info!(sensor = name, socket = %socket, "Launching sensor");
    let mut cmd = Command::new(name);
    cmd.arg("--socket")
        .arg(&socket)
        .arg("--log-level")
        .arg(&cfg.log_level);
    if name == "whisperd" {
        if let Some(model) = &cfg.whisper_model {
            cmd.arg("--whisper-model").arg(model);
        }
    }
    for arg in &cfg.args {
        cmd.arg(arg);
    }
    let child = cmd.spawn()?;
    Ok(child)
}
