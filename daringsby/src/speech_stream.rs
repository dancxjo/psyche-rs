use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
};
use bytes::Bytes;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::broadcast::Receiver;

const SAMPLE_RATE: u32 = 16_000;
const FRAME_MS: usize = 10;
const SILENCE_BYTES: usize = (SAMPLE_RATE as usize / 1000 * FRAME_MS) * 2;
static SILENCE: Lazy<[u8; SILENCE_BYTES]> = Lazy::new(|| [0u8; SILENCE_BYTES]);

/// WebSocket streamer for mouth audio.
///
/// Exposes a single `/` route which upgrades to a WebSocket connection.
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
        Router::new().route(
            "/",
            get({
                let stream = self.clone();
                move |ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(move |sock| stream.clone().stream_audio(sock))
                }
            }),
        )
    }

    async fn stream_audio(self: Arc<Self>, mut socket: WebSocket) {
        let rx = self.tts_rx.clone();
        let mut rx = rx.lock().await;
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Ok(bytes)) => {
                    if !bytes.is_empty()
                        && socket.send(Message::Binary(bytes.to_vec())).await.is_err()
                    {
                        break;
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => {
                    if socket
                        .send(Message::Binary(SILENCE.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::broadcast;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

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
        let url = format!("ws://{addr}");
        let (mut ws, _) = connect_async(url).await.unwrap();
        tx.send(Bytes::from_static(b"A")).unwrap();
        drop(tx);
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg, WsMessage::Binary(b"A".to_vec()));
    }

    /// The router upgrades connections to WebSocket.
    #[tokio::test]
    async fn upgrades_via_router() {
        let (_tx, rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx));
        let addr = start_server(stream).await;
        let url = format!("ws://{addr}");
        let (_ws, _) = connect_async(url).await.unwrap();
    }

    /// When idle the stream emits silence bytes.
    #[tokio::test]
    async fn emits_silence_when_idle() {
        let (_tx, rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx));
        let addr = start_server(stream.clone()).await;
        let url = format!("ws://{addr}");
        let (mut ws, _) = connect_async(url).await.unwrap();
        let first = ws.next().await.unwrap().unwrap();
        assert_eq!(first, WsMessage::Binary(SILENCE.to_vec()));
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(150), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(chunk, WsMessage::Binary(SILENCE.to_vec()));
    }
}
