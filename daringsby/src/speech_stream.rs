use crate::speech_segment::SpeechSegment;
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
/// - `/speech-audio-out` upgrades the connection to a WebSocket and streams the
///   PCM bytes.
///
/// Bytes received from the provided [`Receiver`] are forwarded to connected
/// clients as binary messages. Silence frames are emitted when idle.
pub struct SpeechStream {
    tts_rx: Arc<Mutex<Receiver<Bytes>>>,
    text_rx: Arc<Mutex<Receiver<String>>>,
    text_tx: Sender<String>,
    segment_rx: Arc<Mutex<Receiver<SpeechSegment>>>,
    heard_tx: Sender<String>,
    user_tx: Sender<String>,
}

impl SpeechStream {
    /// Create a new streamer from the given broadcast receivers.
    pub fn new(
        audio_rx: Receiver<Bytes>,
        text_rx: Receiver<String>,
        segment_rx: Receiver<SpeechSegment>,
    ) -> Self {
        let (heard_tx, _) = broadcast::channel(32);
        let (user_tx, _) = broadcast::channel(32);
        let (text_tx, text_rx_out) = broadcast::channel(32);
        let stream = Self {
            tts_rx: Arc::new(Mutex::new(audio_rx)),
            text_rx: Arc::new(Mutex::new(text_rx_out)),
            text_tx: text_tx.clone(),
            segment_rx: Arc::new(Mutex::new(segment_rx)),
            heard_tx: heard_tx.clone(),
            user_tx,
        };

        tokio::spawn(async move {
            let mut src = text_rx;
            while let Ok(t) = src.recv().await {
                let desc = format!("text {} bytes", t.len());
                if let Err(e) = text_tx.send(t.clone()) {
                    warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
                }
                if let Err(e) = heard_tx.send(t) {
                    warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
                }
            }
        });

        stream
    }

    /// Build an [`axum::Router`] exposing the WebSocket streaming routes.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/", get(Self::index))
            .route(
                "/speech-audio-out",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().stream_audio(sock))
                    }
                }),
            )
            .route(
                "/speech-text-out",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().stream_text(sock))
                    }
                }),
            )
            .route(
                "/speech-segments-out",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().stream_segments(sock))
                    }
                }),
            )
            .route(
                "/speech-text-self-in",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().receive_heard(sock))
                    }
                }),
            )
            .route(
                "/speech-text-user-in",
                get({
                    let stream = self.clone();
                    move |ws: WebSocketUpgrade| async move {
                        ws.on_upgrade(move |sock| stream.clone().receive_user(sock))
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

    async fn stream_segments(self: Arc<Self>, mut socket: WebSocket) {
        let rx = self.segment_rx.clone();
        let mut rx = rx.lock().await;
        while let Ok(seg) = rx.recv().await {
            match serde_json::to_string(&seg) {
                Ok(json) => {
                    if socket.send(Message::Text(json)).await.is_err() {
                        error!("websocket segment send failed");
                        break;
                    }
                }
                Err(e) => {
                    error!(error=?e, "segment serialize failed");
                }
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
                    let desc = format!("heard text {} bytes", t.len());
                    if let Err(e) = self.heard_tx.send(t) {
                        warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
                    }
                }
                Message::Binary(b) => {
                    if let Ok(t) = String::from_utf8(b) {
                        let desc = format!("heard text {} bytes", t.len());
                        if let Err(e) = self.heard_tx.send(t) {
                            warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }

    async fn receive_user(self: Arc<Self>, mut socket: WebSocket) {
        while let Some(Ok(msg)) = socket.next().await {
            match msg {
                Message::Text(t) => {
                    let desc = format!("user text {} bytes", t.len());
                    if let Err(e) = self.user_tx.send(t) {
                        warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
                    }
                }
                Message::Binary(b) => {
                    if let Ok(t) = String::from_utf8(b) {
                        let desc = format!("user text {} bytes", t.len());
                        if let Err(e) = self.user_tx.send(t) {
                            warn!(target: "speech_stream", error=?e, what=%desc, "broadcast send failed");
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

    /// Subscribe to user text messages.
    pub fn subscribe_user(&self) -> Receiver<String> {
        self.user_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use futures::{SinkExt, StreamExt};
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
        let (_seg_tx, seg_rx) = broadcast::channel(4);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/speech-audio-out");
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
        let (_seg_tx, seg_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let app = stream.router();
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("ws://"));
        assert!(html.contains("/speech-segments-out"));
        assert!(html.contains("id=\"start\""));
    }

    /// The router upgrades connections to WebSocket.
    #[tokio::test]
    async fn upgrades_via_router() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let (_seg_tx, seg_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let addr = start_server(stream).await;
        let url = format!("ws://{addr}/speech-audio-out");
        let (_ws, _) = connect_async(url).await.unwrap();
    }

    /// When idle the stream does not emit any audio frames.
    #[tokio::test]
    async fn does_not_emit_when_idle() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let (_seg_tx, seg_rx) = broadcast::channel(4);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/speech-audio-out");
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
        let (_seg_tx, seg_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/speech-audio-out");
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

    /// Messages sent by the client are forwarded to subscribers.
    #[tokio::test]
    async fn forwards_user_messages() {
        let (_tx, rx) = broadcast::channel(1);
        let (_t, txt_rx) = broadcast::channel(1);
        let (_seg_tx, seg_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx, txt_rx, seg_rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}/speech-text-user-in");
        let (mut ws, _) = connect_async(url).await.unwrap();
        let mut rx_user = stream.subscribe_user();
        ws.send(WsMessage::Text("hi".into())).await.unwrap();
        let msg = rx_user.recv().await.unwrap();
        assert_eq!(msg, "hi");
    }

    /// Text spoken by the mouth is broadcast as heard self.
    #[tokio::test]
    async fn forwards_mouth_text_to_heard() {
        let (_a_tx, a_rx) = broadcast::channel(1);
        let (t_tx, t_rx) = broadcast::channel(1);
        let (_s_tx, s_rx) = broadcast::channel(1);
        let stream = SpeechStream::new(a_rx, t_rx, s_rx);
        let mut heard = stream.subscribe_heard();

        t_tx.send("hello".to_string()).unwrap();

        let text = tokio::time::timeout(std::time::Duration::from_millis(50), heard.recv())
            .await
            .expect("no heard text")
            .unwrap();

        assert_eq!(text, "hello");
    }
}
