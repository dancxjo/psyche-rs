use async_trait::async_trait;

/// Result of a transcription pass.
///
/// The [`stable`] portion is safe to commit to the conversation while the
/// [`fuzzy`] tail may still change as more audio is processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptResult {
    /// Tokens that are considered temporally stable.
    pub stable: String,
    /// Most recent tokens that may still change.
    pub fuzzy: Option<String>,
}

/// Speech recognition interface.
#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    /// Process a buffer of audio samples.
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()>;

    /// Attempt to transcribe any buffered audio into text.
    async fn try_transcribe(&self) -> anyhow::Result<Option<TranscriptResult>>;
}

/// [`SpeechRecognizer`] implementation that just logs incoming audio.
pub struct DummyRecognizer;

#[async_trait]
impl SpeechRecognizer for DummyRecognizer {
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()> {
        tracing::info!("audio_samples = {}", samples.len());
        Ok(())
    }

    async fn try_transcribe(&self) -> anyhow::Result<Option<TranscriptResult>> {
        Ok(None)
    }
}
