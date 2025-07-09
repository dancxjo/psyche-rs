use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};
use std::sync::Arc;

/// Minimal WebSocket canvas stream placeholder.
///
/// Serves the `memory_viz.html` page at `/canvas`.
#[derive(Default)]
pub struct CanvasStream;

impl CanvasStream {
    async fn index() -> impl IntoResponse {
        Html(include_str!("memory_viz.html"))
    }

    /// Build a router exposing the canvas page.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route("/canvas", get(Self::index))
    }
}
