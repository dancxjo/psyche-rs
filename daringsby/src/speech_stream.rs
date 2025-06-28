use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
};
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver};

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
    tts_rx: Arc<tokio::sync::Mutex<Receiver<Bytes>>>,
}

impl SpeechStream {
    /// Create a new streamer from the given broadcast receiver.
    pub fn new(rx: Receiver<Bytes>) -> Self {
        Self {
            tts_rx: Arc::new(tokio::sync::Mutex::new(rx)),
        }
    }

    /// Build an [`axum::Router`] exposing the WebSocket streaming route.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route("/", get(Self::index)).route(
            "/ws/audio/out",
            get({
                let stream = self.clone();
                move |ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(move |sock| stream.clone().stream_audio(sock))
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
                    if !bytes.is_empty()
                        && socket.send(Message::Binary(bytes.to_vec())).await.is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
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
        let stream = Arc::new(SpeechStream::new(rx));
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
        let stream = Arc::new(SpeechStream::new(rx));
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
        let stream = Arc::new(SpeechStream::new(rx));
        let addr = start_server(stream).await;
        let url = format!("ws://{addr}/ws/audio/out");
        let (_ws, _) = connect_async(url).await.unwrap();
    }

    /// When idle the stream does not emit any audio frames.
    #[tokio::test]
    async fn does_not_emit_when_idle() {
        let (_tx, rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx));
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
        let stream = Arc::new(SpeechStream::new(rx));
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
