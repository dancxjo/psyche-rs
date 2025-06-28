use axum::{
    Router,
    body::Body,
    response::{IntoResponse, Response},
    routing::get,
};
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::broadcast::Receiver;

/// HTTP streamer for mouth audio.
///
/// This type exposes a router serving two routes:
/// - `/` an HTML page with an `<audio>` element.
/// - `/speech.wav` streaming WAV bytes as they arrive from a TTS backend.
///
/// A [`Receiver`] is provided at construction and any bytes received are
/// forwarded directly to the HTTP client.
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

    /// Build an [`axum::Router`] exposing the streaming and index routes.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/", get(Self::index))
            .route("/speech.wav", get(move || self.clone().stream_audio()))
    }

    async fn index() -> impl IntoResponse {
        const INDEX: &str = r#"<!DOCTYPE html>
<html lang=\"en\">
<body>
<audio controls autoplay>
  <source src=\"/speech.wav\" type=\"audio/wav\">
  Your browser does not support the audio element.
</audio>
</body>
</html>
"#;
        axum::response::Html(INDEX)
    }

    async fn stream_audio(self: Arc<Self>) -> Response {
        let rx = self.tts_rx.clone();
        let stream = async_stream::stream! {
            let mut rx = rx.lock().await;
            while let Ok(bytes) = rx.recv().await {
                yield Ok::<Bytes, std::io::Error>(bytes);
            }
        };
        Response::builder()
            .header("Content-Type", "audio/wav")
            .body(Body::from_stream(stream))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::Request};
    use http_body_util::BodyExt;
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    /// When bytes are sent on the channel they stream to the client.
    #[tokio::test]
    async fn streams_bytes_to_client() {
        let (tx, rx) = broadcast::channel(4);
        let stream = Arc::new(SpeechStream::new(rx));
        let app = stream.router();

        let handle = tokio::spawn(async move {
            let req = Request::builder()
                .uri("/speech.wav")
                .body(Body::empty())
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), axum::http::StatusCode::OK);
            let body = resp.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body.as_ref(), b"A");
        });

        tx.send(Bytes::from_static(b"A")).unwrap();
        drop(tx);
        handle.await.unwrap();
    }

    /// The index route serves an HTML page with an audio element.
    #[tokio::test]
    async fn serves_index_html() {
        let (_tx, rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(rx));
        let app = stream.router();
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body).unwrap().contains("<audio"));
    }
}
