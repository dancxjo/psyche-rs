use async_trait::async_trait;
use futures_util::TryStreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_util::io::StreamReader;

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

/// Chunk of audio data streamed from [`CoquiMouth`].
#[derive(Debug, PartialEq, Eq)]
pub enum AudioChunk {
    /// Raw bytes of WAV audio.
    Data(Vec<u8>),
    /// Marker indicating the end of an utterance.
    Done,
}

/// [`Mouth`] implementation that proxies text to a local Coqui TTS server and
/// streams the resulting audio back in small pieces.
pub struct CoquiMouth {
    /// Channel used to forward audio chunks.
    pub tx: mpsc::Sender<AudioChunk>,
    /// Base URL of the Coqui server, e.g. `http://localhost:5002`.
    pub endpoint: String,
    /// Identifier of the desired language model.
    pub language_id: String,
    /// Identifier of the speaker voice to synthesize with.
    pub speaker_id: String,
}

#[async_trait(?Send)]
impl Mouth for CoquiMouth {
    async fn say(&self, phrase: &str) -> anyhow::Result<()> {
        let segmenter =
            pragmatic_segmenter::Segmenter::new().map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let client = Client::new();

        for sentence in segmenter.segment(phrase) {
            tracing::info!("Speaking sentence: {}", sentence);
            let resp = client
                .post(format!("{}/tts", self.endpoint))
                .json(&json!({
                    "text": sentence,
                    "speaker_id": self.speaker_id,
                    "language_id": self.language_id,
                }))
                .send()
                .await?;

            let stream = resp
                .bytes_stream()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
            let mut reader = StreamReader::new(stream);
            let mut buf = [0u8; 512];
            loop {
                let n = reader.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                self.tx
                    .send(AudioChunk::Data(buf[..n].to_vec()))
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            }
        }

        self.tx
            .send(AudioChunk::Done)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        Ok(())
    }
}
