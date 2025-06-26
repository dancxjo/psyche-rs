use async_trait::async_trait;

/// Speech recognition interface.
#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    /// Process a buffer of audio samples.
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()>;
}

/// [`SpeechRecognizer`] implementation that just logs incoming audio.
pub struct DummyRecognizer;

#[async_trait]
impl SpeechRecognizer for DummyRecognizer {
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()> {
        tracing::info!("audio_samples = {}", samples.len());
        Ok(())
    }
}
