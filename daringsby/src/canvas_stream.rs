use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver, Sender};

/// WebSocket streamer for canvas snapshots.
///
/// The server broadcasts "snap" commands to connected clients and
/// receives JPEG image bytes in response.
pub struct CanvasStream {
    tx: Sender<Vec<u8>>, // image bytes
    cmd: Sender<String>, // commands like "snap"
}
impl Default for CanvasStream {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(8);
        let (cmd, _) = broadcast::channel(8);
        Self { tx, cmd }
    }
}

impl CanvasStream {
    /// Subscribe to incoming images.
    pub fn subscribe(&self) -> Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    /// Request a snapshot from all connected clients.
    pub fn request_snap(&self) {
        let _ = self.cmd.send("snap".into());
    }

    /// Build a router exposing the canvas WebSocket endpoint.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route(
            "/canvas-jpeg-in",
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
                        let _ = self.tx.send(data);
                    }
                }
                Ok(cmd) = cmd_rx.recv() => {
                    if socket.send(Message::Text(cmd)).await.is_err() {
                        break;
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

    async fn start_server(stream: Arc<CanvasStream>) -> std::net::SocketAddr {
        let app = stream.router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[tokio::test]
    async fn forwards_images_and_commands() {
        let stream = Arc::new(CanvasStream::default());
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/canvas-jpeg-in");
        let (mut ws, _) = connect_async(url).await.unwrap();
        let mut rx = stream.subscribe();
        ws.send(WsMessage::Binary(vec![4, 5, 6])).await.unwrap();
        let img = rx.recv().await.unwrap();
        assert_eq!(img, vec![4, 5, 6]);
        stream.request_snap();
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Text("snap".into()));
    }
}
