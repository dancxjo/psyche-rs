use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use psyche::llm::{CanChat, LlmProfile};
use roxmltree::Document;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, trace};

/// Mapping of available motors.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct WouldConfig {
    #[serde(default)]
    pub motors: HashMap<String, String>,
}

impl WouldConfig {
    /// Load configuration from a TOML file containing a `[would.motors]` table.
    pub async fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        #[derive(serde::Deserialize)]
        struct Root {
            #[serde(default)]
            would: WouldConfig,
        }
        let text = tokio::fs::read_to_string(path).await?;
        let root: Root = toml::from_str(&text)?;
        Ok(root.would)
    }
}

/// Parsed function call from the LLM.
#[derive(Debug)]
struct FnCall {
    name: String,
    args: HashMap<String, String>,
    body: String,
}

fn parse_function(xml: &str) -> anyhow::Result<FnCall> {
    let doc = Document::parse(xml)?;
    let root = doc.root_element();
    let mut args = HashMap::new();
    for attr in root.attributes() {
        args.insert(attr.name().to_string(), attr.value().to_string());
    }
    let body = root.text().unwrap_or("").trim().to_string();
    Ok(FnCall {
        name: root.tag_name().name().to_string(),
        args,
        body,
    })
}

async fn interpret_urge(
    llm: &(dyn CanChat + Sync),
    profile: &LlmProfile,
    urge: &str,
) -> anyhow::Result<FnCall> {
    const SYSTEM: &str = "Convert the urge into a single <function> call.";
    let mut stream = llm.chat_stream(profile, SYSTEM, urge).await?;
    let mut resp = String::new();
    while let Some(tok) = stream.next().await {
        trace!(target = "llm", token = %tok, "stream token");
        resp.push_str(&tok);
    }
    debug!(target = "llm", response = %resp, "llm response");
    parse_function(&resp)
}

async fn run_motor(path: &str, call: FnCall, output_sock: Option<PathBuf>) -> anyhow::Result<()> {
    let mut cmd = Command::new(path);
    for (k, v) in &call.args {
        cmd.arg(format!("--{}", k)).arg(v);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    info!(motor = %call.name, path, args = ?call.args, "executing motor");
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(call.body.as_bytes()).await?;
    }
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();
    while let Some(line) = reader.next_line().await? {
        info!(response = %line, "motor response");
        if let Some(sock) = &output_sock {
            if let Ok(mut s) = UnixStream::connect(sock).await {
                s.write_all(b"/response\n").await.ok();
                s.write_all(line.as_bytes()).await.ok();
                s.write_all(b"\n---\n").await.ok();
                s.shutdown().await.ok();
            }
        }
    }
    let status = child.wait().await?;
    if !status.success() {
        error!(status = ?status, "motor exited with error");
    }
    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    cfg: WouldConfig,
    llm: std::sync::Arc<dyn CanChat + Sync + Send>,
    profile: LlmProfile,
    output_sock: Option<PathBuf>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_to_string(&mut buf).await?;
    if buf.trim().is_empty() {
        return Ok(());
    }
    info!(urge = %buf, "urge received");
    let call = interpret_urge(llm.as_ref(), &profile, &buf).await?;
    if let Some(path) = cfg.motors.get(&call.name) {
        run_motor(path, call, output_sock).await?;
    } else {
        error!(motor = %call.name, "motor not found");
    }
    Ok(())
}

/// Run the would daemon.
pub async fn run(
    socket: PathBuf,
    config: PathBuf,
    output_sock: Option<PathBuf>,
    llm: std::sync::Arc<dyn CanChat + Sync + Send>,
    profile: LlmProfile,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    let cfg = WouldConfig::load(config).await?;
    info!(?socket, "would listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let cfg = cfg.clone();
        let llm = llm.clone();
        let profile = profile.clone();
        let out = output_sock.clone();
        tokio::task::spawn_local(async move {
            if let Err(e) = handle_connection(stream, cfg, llm, profile, out).await {
                error!(?e, "connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psyche::llm::mock_chat::NamedMockChat;
    use tempfile::tempdir;
    use tokio::task::LocalSet;

    #[tokio::test]
    async fn executes_motor_and_forwards_output() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("would.sock");
        let out_sock = dir.path().join("out.sock");
        let config_path = dir.path().join("config.toml");
        tokio::fs::write(&config_path, "[would.motors]\necho = \"/bin/echo\"\n")
            .await
            .unwrap();
        let llm = NamedMockChat {
            name: "<echo/>".into(),
        };
        let llm = std::sync::Arc::new(llm);
        let profile = LlmProfile {
            provider: "mock".into(),
            model: "mock".into(),
            capabilities: vec![],
        };
        let local = LocalSet::new();
        let handle = local.spawn_local(run(
            sock.clone(),
            config_path.clone(),
            Some(out_sock.clone()),
            llm,
            profile,
        ));
        local
            .run_until(async {
                let listener = UnixListener::bind(&out_sock).unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut client = UnixStream::connect(&sock).await.unwrap();
                client.write_all(b"test\n").await.unwrap();
                client.shutdown().await.unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = String::new();
                BufReader::new(&mut stream)
                    .read_line(&mut buf)
                    .await
                    .unwrap();
                assert!(buf.contains("/response"));
            })
            .await;
        handle.abort();
    }
}
