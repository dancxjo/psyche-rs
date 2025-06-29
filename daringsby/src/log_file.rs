use std::path::PathBuf;

/// Returns the path to Pete's motor log file.
///
/// The path can be overridden for tests by setting the
/// `DARINGSBY_MOTOR_LOG_PATH` environment variable.
pub fn motor_log_path() -> PathBuf {
    std::env::var("DARINGSBY_MOTOR_LOG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/motor_log.txt")))
}
