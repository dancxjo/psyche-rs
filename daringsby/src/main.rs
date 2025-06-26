use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::serve;
use axum::{
    Extension, Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Html,
    routing::get,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use psyche_rs::speech::{DummyRecognizer, SpeechRecognizer, TranscriptResult};
use serde_json::json;
mod whisper_recognizer;
use std::pin::Pin;
use tokio::net::TcpListener;
use tokio_util::{
    codec::{FramedRead, LengthDelimitedCodec},
    io::StreamReader,
};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use whisper_recognizer::WhisperRecognizer;

async fn asr_ws(
    ws: WebSocketUpgrade,
    Extension(tx): Extension<tokio::sync::broadcast::Sender<TranscriptResult>>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| asr_stream(socket, tx.subscribe()))
}

async fn asr_stream(
    mut socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<TranscriptResult>,
) {
    while let Ok(tr) = rx.recv().await {
        let msg = json!({
            "stable": tr.stable,
            "fuzzy": tr.fuzzy,
        })
        .to_string();
        if socket.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}

async fn asr_view() -> Html<&'static str> {
    Html(include_str!("../static/asr_view.html"))
}

async fn audio_in(
    ws: WebSocketUpgrade,
    Extension(recognizer): Extension<Arc<dyn SpeechRecognizer>>,
    Extension(tx): Extension<tokio::sync::broadcast::Sender<TranscriptResult>>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_socket(socket, recognizer, tx))
}

async fn handle_socket(
    socket: WebSocket,
    recognizer: Arc<dyn SpeechRecognizer>,
    tx: tokio::sync::broadcast::Sender<TranscriptResult>,
) {
    let stream = stream::unfold(socket, |mut socket| async {
        match socket.recv().await {
            Some(Ok(Message::Binary(b))) => Some((Ok(Bytes::from(b)), socket)),
            Some(Ok(_)) => Some((
                Err(std::io::Error::new(std::io::ErrorKind::Other, "non-binary")),
                socket,
            )),
            Some(Err(e)) => Some((
                Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                socket,
            )),
            None => None,
        }
    });
    let stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
        Box::pin(stream);

    let reader = StreamReader::new(stream);
    let mut framed = FramedRead::new(reader, LengthDelimitedCodec::new());
    let mut last = Instant::now();
    while let Some(frame) = framed.next().await {
        match frame {
            Ok(bytes) => {
                let samples: Vec<i16> = bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
                let now = Instant::now();
                info!(
                    size = samples.len(),
                    ms = now.duration_since(last).as_millis()
                );
                last = now;
                if let Err(e) = recognizer.recognize(&samples).await {
                    error!("recognize error: {:?}", e);
                }
                match recognizer.try_transcribe().await {
                    Ok(Some(tr)) => {
                        let _ = tx.send(tr.clone());
                        info!(stable = %tr.stable, fuzzy = ?tr.fuzzy);
                    }
                    Ok(None) => {}
                    Err(e) => error!("transcribe error: {:?}", e),
                }
            }
            Err(e) => {
                error!("frame error: {:?}", e);
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let recognizer: Arc<dyn SpeechRecognizer> = if let Ok(path) = std::env::var("WHISPER_MODEL") {
        match WhisperRecognizer::new(&path) {
            Ok(r) => Arc::new(r),
            Err(e) => {
                error!("failed to init whisper: {:?}", e);
                Arc::new(DummyRecognizer)
            }
        }
    } else {
        Arc::new(DummyRecognizer)
    };
    let (tx, _rx) = tokio::sync::broadcast::channel::<TranscriptResult>(16);
    let app = Router::new()
        .route("/audio/in", get(audio_in))
        .route("/debug/asr", get(asr_ws))
        .route("/debug/asr/view", get(asr_view))
        .route("/", get(|| async { "ok" }))
        .layer(Extension(recognizer))
        .layer(Extension(tx));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("signal received, shutting down");
}
