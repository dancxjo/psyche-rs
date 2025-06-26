use async_trait::async_trait;

/// Abstraction for outputting spoken text.
///
/// Implementations may route text to a TTS engine, a console log or any other
/// speaking mechanism.
///
/// # Example
///
/// ```no_run
/// use psyche_rs::mouth::{Mouth};
/// use async_trait::async_trait;
///
/// struct ConsoleMouth;
///
/// #[async_trait(?Send)]
/// impl Mouth for ConsoleMouth {
///     async fn say(&self, phrase: &str) -> anyhow::Result<()> {
///         println!("{}", phrase);
///         Ok(())
///     }
/// }
/// ```
#[async_trait(?Send)]
pub trait Mouth: Send + Sync {
    /// Speak the provided phrase.
    async fn say(&self, phrase: &str) -> anyhow::Result<()>;
}

/// [`Mouth`] implementation that logs spoken phrases using [`tracing`].
pub struct DummyMouth;

#[async_trait(?Send)]
impl Mouth for DummyMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        tracing::info!("say: {}", phrase);
        Ok(())
    }
}

/// [`Mouth`] implementation that stores spoken phrases for later inspection.
/// This is primarily useful for tests and examples where actual speech
/// synthesis isn't required.
#[derive(Clone)]
pub struct LoggingMouth {
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LoggingMouth {
    /// Create a new mouth with an associated log vector.
    ///
    /// # Example
    /// ```no_run
    /// use psyche_rs::mouth::{LoggingMouth, Mouth};
    /// #[tokio::main]
    /// async fn main() {
    ///     let (mouth, log) = LoggingMouth::new();
    ///     mouth.say("hello world").await.unwrap();
    ///     assert_eq!(log.0.lock().unwrap()[0], "hello world");
    /// }
    /// ```
    pub fn new() -> (Self, LoggingMouthLog) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (Self { log: log.clone() }, LoggingMouthLog(log))
    }
}

/// Shared log type returned by [`LoggingMouth::new`].
#[derive(Clone)]
pub struct LoggingMouthLog(pub std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl LoggingMouthLog {
    /// Return the last phrase spoken, if any.
    pub fn last(&self) -> Option<String> {
        self.0.lock().ok().and_then(|v| v.last().cloned())
    }
}

#[async_trait(?Send)]
impl Mouth for LoggingMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        if let Ok(mut log) = self.log.lock() {
            log.push(phrase.to_string());
        }
        Ok(())
    }
}
