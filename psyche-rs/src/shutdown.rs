//! Graceful shutdown helpers.
//!
//! `shutdown_signal` waits for either `Ctrl+C` or a `SIGTERM` on Unix.
//! This can be used by binaries and tests to terminate long running
//! tasks.
//!
//! ```no_run
//! # async fn example() {
//! use psyche_rs::shutdown_signal;
//! shutdown_signal().await;
//! # }
//! ```

/// Waits for either `Ctrl+C` or `SIGTERM` (on Unix) to be received.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
