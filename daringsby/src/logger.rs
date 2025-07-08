use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::log_sensation_layer::LogSensationLayer;
use psyche_rs::Sensation;

/// Initializes tracing using the `RUST_LOG` environment variable.
///
/// If `RUST_LOG` is not set or fails to parse, logging defaults to the `debug`
/// level. This function is intended for binaries; tests should prefer
/// [`try_init`] to avoid panicking if the subscriber is already set.
///
/// # Examples
///
/// ```no_run
/// use daringsby::logger;
/// unsafe { std::env::set_var("RUST_LOG", "info") };
/// logger::try_init().expect("logger initialized");
/// ```
pub fn init() {
    try_init().expect("failed to initialize tracing")
}

/// Attempts to initialize tracing and returns an error if a subscriber is
/// already set.
pub fn try_init() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    fmt().with_env_filter(filter).try_init().map_err(Into::into)
}

/// Initializes tracing with an additional [`LogSensationLayer`].
pub fn try_init_with_sender(
    tx: UnboundedSender<Vec<Sensation<String>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .with(LogSensationLayer::new(tx))
        .try_init()
        .map_err(Into::into)
}
