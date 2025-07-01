use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::{CanvasStream, SpeechStream, VisionSensor, args::Args};

/// Run the HTTP server exposing speech and vision streams.
pub async fn run_server(
    stream: Arc<SpeechStream>,
    vision: Arc<VisionSensor>,
    canvas: Arc<CanvasStream>,
    args: &Args,
) -> JoinHandle<()> {
    let app = stream
        .clone()
        .router()
        .merge(vision.clone().router())
        .merge(canvas.clone().router());
    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("invalid addr");
    tokio::spawn(async move {
        tracing::info!(%addr, "serving speech stream");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind TcpListener");
        axum::serve(listener, app).await.expect("axum serve failed");
    })
}
