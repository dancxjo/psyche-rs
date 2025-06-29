use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
};
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::{
    Mutex,
    broadcast::{self, Receiver, Sender},
};
use tracing::{error, warn};

/// WebSocket streamer for mouth audio.
///
/// Exposes two routes:
/// - `/` serves a minimal HTML page that connects to the WebSocket and plays
///   PCM data via the Web Audio API.
/// - `/ws/audio/out` upgrades the connection to a WebSocket and streams the PCM
///   bytes.
///
/// Bytes received from the provided [`Receiver`] are forwarded to connected
/// clients as binary messages. Silence frames are emitted when idle.
pub struct SpeechStream {
    tts_rx: Arc<Mutex<Receiver<Bytes>>>,
    text_rx: Arc<Mutex<Receiver<String>>>,
    heard_tx: Sender<String>,
}

impl SpeechStream {
    /// Create a new streamer from the given broadcast receivers.
    pub fn new(audio_rx: Receiver<Bytes>, text_rx: Receiver<String>) -> Self {
        let (heard_tx, _) = broadcast::channel(32);
        Self {
            tts_rx: Arc::new(Mutex::new(audio_rx)),
            text_rx: Arc::new(Mutex::new(text_rx)),
            heard_tx,
        }
    }

    /// Build an [`axum::Router`] exposing the WebSocket streaming routes.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/", get(Self::index))
            .route(
                "/ws/audio/out",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().stream_audio(sock))
                    }
                }),
            )
            .route(
                "/ws/audio/text/out",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().stream_text(sock))
                    }
                }),
            )
            .route(
                "/ws/audio/self/in",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().receive_heard(sock))
                    }
                }),
            )
    }

    async fn index() -> impl IntoResponse {
        const INDEX: &str = include_str!("index.html");
        Html(INDEX)
    }

    async fn stream_audio(self: Arc<Self>, mut socket: WebSocket) {
        let rx = self.tts_rx.clone();
        let mut rx = rx.lock().await;
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if !bytes.is_empty() {
                        if socket.send(Message::Binary(bytes.to_vec())).await.is_err() {
                            error!("websocket audio send failed");
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!(%count, "audio channel lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn stream_text(self: Arc<Self>, mut socket: WebSocket) {
        let rx = self.text_rx.clone();
        let mut rx = rx.lock().await;
        loop {
            match rx.recv().await {
                Ok(text) => {
                    if socket.send(Message::Text(text)).await.is_err() {
                        error!("websocket text send failed");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!(%count, "text channel lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn receive_heard(self: Arc<Self>, mut socket: WebSocket) {
        while let Some(Ok(msg)) = socket.next().await {
            match msg {
                Message::Text(t) => {
                    if self.heard_tx.send(t).is_err() {
                        warn!("heard_tx receiver dropped");
                    }
                }
                Message::Binary(b) => {
                    if let Ok(t) = String::from_utf8(b) {
                        if self.heard_tx.send(t).is_err() {
                            warn!("heard_tx receiver dropped");
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }

    /// Subscribe to heard-back text messages.
    pub fn subscribe_heard(&self) -> Receiver<String> {
        self.heard_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use futures::StreamExt;
    use http_body_util::BodyExt;
    use tokio::sync::broadcast;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
    use tower::ServiceExt;

    async fn start_server(stream: Arc<SpeechStream>) -> std::net::SocketAddr {
        let app = stream.router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    /// When bytes are sent on the channel they stream to the client.
    #[tokio::test]
    async fn streams_bytes_to_client() {
        let (tx, rx) = broadcast::channel(4);
        let (_txt_tx, txt_rx) = broadcast::channel(4);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/ws/audio/out");
        let (mut ws, _) = connect_async(url).await.unwrap();
        tx.send(Bytes::from_static(b"A")).unwrap();
        drop(tx);
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Binary(b"A".to_vec()));
    }

    /// The index route serves HTML with WebSocket playback code.
    #[tokio::test]
    async fn serves_index_html() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx));
        let app = stream.router();
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("ws://"));
        assert!(html.contains("/ws/audio/out"));
        assert!(html.contains("id=\"start\""));
    }

    /// The router upgrades connections to WebSocket.
    #[tokio::test]
    async fn upgrades_via_router() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx));
        let addr = start_server(stream).await;
        let url = format!("ws://{addr}/ws/audio/out");
        let (_ws, _) = connect_async(url).await.unwrap();
    }

    /// When idle the stream does not emit any audio frames.
    #[tokio::test]
    async fn does_not_emit_when_idle() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/ws/audio/out");
        let (mut ws, _) = connect_async(url).await.unwrap();
        let recv = tokio::time::timeout(std::time::Duration::from_millis(150), ws.next()).await;
        assert!(recv.is_err(), "stream should be silent when idle");
        drop(ws);
    }

    /// Lagged broadcast messages do not close the connection.
    #[tokio::test]
    async fn continues_after_lagged_message() {
        let (tx, rx) = broadcast::channel(4);
        let (_t, txt_rx) = broadcast::channel(4);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/ws/audio/out");
        let (mut ws, _) = connect_async(url).await.unwrap();
        // exceed capacity quickly
        for _ in 0..10 {
            tx.send(Bytes::from_static(b"A")).unwrap();
        }
        tx.send(Bytes::from_static(b"Z")).unwrap();
        drop(tx);
        let mut got_z = false;
        while let Some(msg) = ws.next().await {
            let m = msg.unwrap();
            if m == WsMessage::Binary(b"Z".to_vec()) {
                got_z = true;
                break;
            }
        }
        assert!(got_z);
    }
}
