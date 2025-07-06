use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::warn;

/// WebSocket streamer for webcam snapshots.
///
/// Spawns:
/// - WebSocket session task per client connection
///
/// The server broadcasts "snap" commands to connected clients and
/// receives JPEG image bytes in response.
pub struct VisionSensor {
    tx: Sender<Vec<u8>>, // image bytes
    cmd: Sender<String>, // commands like "snap"
}

impl Default for VisionSensor {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(8);
        let (cmd, _) = broadcast::channel(8);
        Self { tx, cmd }
    }
}

impl VisionSensor {
    /// Subscribe to incoming images.
    pub fn subscribe(&self) -> Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    /// Request a snapshot from all connected clients.
    pub fn request_snap(&self) {
        let desc = "snap command";
        if let Err(e) = self.cmd.send("snap".into()) {
            warn!(target: "vision_sensor", error=?e, what=%desc, "broadcast command send failed");
        }
    }

    /// Build a router exposing the vision WebSocket endpoint.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route(
            "/vision-jpeg-in",
            get(move |ws: WebSocketUpgrade| {
                let stream = self.clone();
                async move { ws.on_upgrade(move |sock| stream.clone().session(sock)) }
            }),
        )
    }

    async fn session(self: Arc<Self>, mut socket: WebSocket) {
        let mut cmd_rx = self.cmd.subscribe();
        loop {
            tokio::select! {
                Some(Ok(msg)) = socket.next() => {
                    if let Message::Binary(data) = msg {
                        let desc = format!("vision snapshot JPEG {} bytes", data.len());
                        if let Err(e) = self.tx.send(data) {
                            warn!(target: "vision_sensor", error=?e, what=%desc, "broadcast send failed");
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Ok(cmd) => {
                            if socket.send(Message::Text(cmd)).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::info!("vision command channel closed, exiting");
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(count)) => {
                            warn!(%count, "vision command channel lagged");
                            continue;
                        }
                    }
                }
                else => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
    use tracing_test::traced_test;

    async fn start_server(stream: Arc<VisionSensor>) -> std::net::SocketAddr {
        let app = stream.router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[tokio::test]
    async fn forwards_images_and_commands() {
        let stream = Arc::new(VisionSensor::default());
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/vision-jpeg-in");
        let (mut ws, _) = connect_async(url).await.unwrap();
        let mut rx = stream.subscribe();
        ws.send(WsMessage::Binary(vec![1, 2, 3])).await.unwrap();
        let img = rx.recv().await.unwrap();
        assert_eq!(img, vec![1, 2, 3]);
        stream.request_snap();
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Text("snap".into()));
    }

    #[traced_test]
    #[tokio::test]
    async fn warns_when_snap_fails() {
        let sensor = VisionSensor::default();
        sensor.request_snap();
        assert!(logs_contain("broadcast command send failed"));
    }
}
