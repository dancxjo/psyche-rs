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
/// The server broadcasts `"snap"` commands to connected clients and
/// receives JPEG image bytes in response. Connected clients receive
/// `"snap"` commands and respond with JPEG bytes.
///
/// This is largely identical to `VisionSensor` but intended for a drawing canvas
/// rather than a webcam.
pub struct CanvasStream {
    tx: Sender<Vec<u8>>,    // Image bytes
    svg_tx: Sender<String>, // SVG data
    cmd: Sender<String>,    // Commands like "snap"
}

impl Default for CanvasStream {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(8);
        let (svg_tx, _) = broadcast::channel(8);
        let (cmd, _) = broadcast::channel(8);
        Self { tx, svg_tx, cmd }
    }
}

impl CanvasStream {
    /// Subscribe to incoming images.
    pub fn subscribe(&self) -> Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    /// Subscribe to outgoing SVG drawings.
    pub fn subscribe_svg(&self) -> Receiver<String> {
        self.svg_tx.subscribe()
    }

    /// Subscribe to outgoing commands (useful for tests or monitoring).
    pub fn subscribe_cmd(&self) -> Receiver<String> {
        self.cmd.subscribe()
    }

    /// Request a snapshot from all connected clients.
    pub fn request_snap(&self) {
        let _ = self.cmd.send("snap".into());
    }

    /// Broadcast an SVG drawing to all connected clients.
    pub fn broadcast_svg(&self, svg: String) {
        let _ = self.svg_tx.send(svg);
    }

    /// Inject an image into the stream. Intended for tests.
    pub fn push_image(&self, img: Vec<u8>) {
        let _ = self.tx.send(img);
    }

    /// Build a router exposing the canvas WebSocket endpoint.
    pub fn router(self: Arc<Self>) -> Router {
        let jpeg_stream = self.clone();
        let svg_stream = self.clone();
        Router::new()
            .route(
                "/canvas-jpeg-in",
                get(move |ws: WebSocketUpgrade| {
                    let stream = jpeg_stream.clone();
                    async move { ws.on_upgrade(move |sock| stream.clone().session(sock)) }
                }),
            )
            .route(
                "/drawing-svg-out",
                get(move |ws: WebSocketUpgrade| {
                    let stream = svg_stream.clone();
                    async move { ws.on_upgrade(move |sock| stream.clone().stream_svg(sock)) }
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

    async fn stream_svg(self: Arc<Self>, mut socket: WebSocket) {
        let mut rx = self.svg_tx.subscribe();
        while let Ok(svg) = rx.recv().await {
            if socket.send(Message::Text(svg)).await.is_err() {
                break;
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
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn forwards_images_and_commands() {
        let stream = Arc::new(CanvasStream::default());
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/canvas-jpeg-in");
        let (mut ws, _) = connect_async(url).await.unwrap();

        let mut rx = stream.subscribe();
        ws.send(WsMessage::Binary(vec![1, 2, 3])).await.unwrap();
        let img = rx.recv().await.unwrap();
        assert_eq!(img, vec![1, 2, 3]);

        stream.request_snap();
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Text("snap".into()));
    }

    #[tokio::test]
    async fn broadcasts_svg_to_clients() {
        let stream = Arc::new(CanvasStream::default());
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/drawing-svg-out");
        let (mut ws, _) = connect_async(url).await.unwrap();

        stream.broadcast_svg("<svg/>".into());
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Text("<svg/>".into()));
    }
}
