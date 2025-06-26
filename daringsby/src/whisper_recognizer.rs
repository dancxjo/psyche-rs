use std::sync::Mutex;

use async_trait::async_trait;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use psyche_rs::speech::SpeechRecognizer;

/// Speech recognizer powered by `whisper-rs`.
///
/// Incoming audio samples are buffered until [`WhisperRecognizer::transcribe`] is
/// called, at which point the buffered audio is fed into the model and
/// the resulting transcript is returned.
pub struct WhisperRecognizer {
    ctx: WhisperContext,
    buffer: Mutex<Vec<i16>>, // collected PCM samples
}

impl WhisperRecognizer {
    /// Load the Whisper model at `model_path`.
    pub fn new(model_path: &str) -> anyhow::Result<Self> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self {
            ctx,
            buffer: Mutex::new(Vec::new()),
        })
    }

    /// Attempt to transcribe the buffered audio.
    pub fn transcribe(&self) -> anyhow::Result<Option<String>> {
        let mut buf = self.buffer.lock().unwrap();
        if buf.is_empty() {
            return Ok(None);
        }

        let mut float_buf = vec![0.0f32; buf.len()];
        whisper_rs::convert_integer_to_float_audio(&buf, &mut float_buf)
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut state = self.ctx.create_state().map_err(|e| anyhow::anyhow!(e))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state
            .full(params, &float_buf)
            .map_err(|e| anyhow::anyhow!(e))?;

        let segments = state.full_n_segments().map_err(|e| anyhow::anyhow!(e))?;
        let mut out = String::new();
        for i in 0..segments {
            out.push_str(
                &state
                    .full_get_segment_text(i)
                    .map_err(|e| anyhow::anyhow!(e))?,
            );
        }
        buf.clear();
        Ok(Some(out))
    }
}

#[async_trait]
impl SpeechRecognizer for WhisperRecognizer {
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()> {
        self.buffer.lock().unwrap().extend_from_slice(samples);
        Ok(())
    }

    async fn try_transcribe(&self) -> anyhow::Result<Option<String>> {
        self.transcribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_model_path_fails() {
        assert!(WhisperRecognizer::new("/no/model/here").is_err());
    }
}
