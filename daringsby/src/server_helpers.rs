use futures::Future;
use psyche_rs::AbortGuard;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::{CanvasStream, SpeechStream, VisionSensor, args::Args};
use axum::Router;

/// Run the HTTP server exposing speech, vision, canvas, and memory graph streams.
pub async fn run_server(
    stream: Arc<SpeechStream>,
    vision: Arc<VisionSensor>,
    canvas: Arc<CanvasStream>,
    memory: Router,
    args: &Args,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> AbortGuard {
    let app = stream
        .clone()
        .router()
        .merge(vision.clone().router())
        .merge(canvas.clone().router())
        .merge(memory);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("invalid addr");

    let handle = tokio::spawn(async move {
        let mut shutdown = Box::pin(shutdown);
        tracing::info!(%addr, "serving HTTP interface");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind TcpListener");

        tokio::select! {
            res = axum::serve(listener, app) => {
                if let Err(e) = res {
                    tracing::error!(?e, "axum serve failed");
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutting down Axum server");
            }
        }
    });

    AbortGuard::new(handle)
}
