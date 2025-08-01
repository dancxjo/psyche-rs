use psyche::models::Sensation;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, trace};
use uuid::Uuid;

/// Continuously read newline-delimited text from `path` and forward each line
/// as a [`Sensation`] with `dest_path`.
pub async fn watch_socket(path: PathBuf, dest_path: String, tx: UnboundedSender<Sensation>) {
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                let mut reader = BufReader::new(stream);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let text = line.trim_end().to_string();
                            if text.is_empty() {
                                continue;
                            }
                            trace!(%text, socket=%path.display(), "pipe line received");
                            let s = Sensation {
                                id: Uuid::new_v4().to_string(),
                                path: dest_path.clone(),
                                text,
                            };
                            if tx.send(s).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            error!(?e, socket=%path.display(), "pipe read error");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!(?e, socket=%path.display(), "failed to connect pipe");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Wait for all `deps` sockets to exist before watching `path` for input.
pub async fn watch_socket_when_ready(
    path: PathBuf,
    dest_path: String,
    tx: UnboundedSender<Sensation>,
    deps: Vec<PathBuf>,
) {
    for dep in deps {
        while !dep.exists() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    watch_socket(path, dest_path, tx).await;
}
