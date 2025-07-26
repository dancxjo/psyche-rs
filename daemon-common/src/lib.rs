use clap::ValueEnum;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl From<LogLevel> for tracing_subscriber::filter::LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing_subscriber::filter::LevelFilter::ERROR,
            LogLevel::Warn => tracing_subscriber::filter::LevelFilter::WARN,
            LogLevel::Info => tracing_subscriber::filter::LevelFilter::INFO,
            LogLevel::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
            LogLevel::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
        }
    }
}

/// Spawn the process as a background daemon when `enable` is true.
///
/// This uses the [`daemonize`](https://docs.rs/daemonize) crate under the
/// hood. In tests or foreground runs pass `false` to skip daemonization.
pub fn maybe_daemonize(enable: bool) -> anyhow::Result<()> {
    if enable {
        daemonize::Daemonize::new()
            .start()
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    Ok(())
}
