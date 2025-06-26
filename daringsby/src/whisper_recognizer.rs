use std::sync::{Arc, Mutex, Weak};

use tokio::sync::Mutex as AsyncMutex;
use tokio::{
    task::JoinHandle,
    time::{Duration, sleep},
};
use tokio_util::sync::CancellationToken;

use async_trait::async_trait;
use num_cpus;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use psyche_rs::speech::{SpeechRecognizer, TranscriptResult};

/// Speech recognizer powered by `whisper-rs`.
///
/// Incoming audio samples are buffered until [`WhisperRecognizer::transcribe`] is
/// called, at which point the buffered audio is fed into the model and
/// the resulting transcript is returned.
pub struct WhisperRecognizer {
    ctx: WhisperContext,
    /// Rolling audio buffer in PCM samples.
    buffer: Mutex<Vec<i16>>, // collected PCM samples
    /// Tokens that have been emitted as stable.
    stable_tokens: Mutex<Vec<String>>,
    /// Tokens from the last transcription (stable + fuzzy).
    last_tokens: Mutex<Vec<String>>,
    /// Latest completed transcript.
    last_result: Mutex<Option<TranscriptResult>>,
    /// Currently running transcription job.
    job: AsyncMutex<Option<Job>>,
    /// Weak pointer to upgrade for scheduling.
    self_ref: Mutex<Weak<WhisperRecognizer>>,
}

struct Job {
    token: CancellationToken,
    handle: JoinHandle<()>,
}

const SAMPLE_RATE: usize = 16_000;
const MAX_DURATION_SECS: usize = 4;
const MAX_SAMPLES: usize = SAMPLE_RATE * MAX_DURATION_SECS;
const DEBOUNCE_MS: u64 = 300;
const MIN_SAMPLES: usize = SAMPLE_RATE / 5; // ~200ms of audio
const STABLE_PROB_THRESHOLD: f32 = 0.85;

/// Promote tokens to stable based on confidence.
///
/// Tokens are examined starting at the current length of `stable`. Any
/// consecutive tokens with probability at or above `threshold` are moved from
/// `tokens` into `stable`.
///
/// # Examples
///
/// ```ignore
/// use daringsby::whisper_recognizer::promote_confident_tokens;
/// let mut stable = vec!["hello".to_string()];
/// let tokens = vec!["hello".to_string(), "world".to_string(), "again".to_string()];
/// let probs = vec![0.99, 0.92, 0.5];
/// let added = promote_confident_tokens(&mut stable, &tokens, &probs, 0.9);
/// assert_eq!(added, 1);
/// assert_eq!(stable, vec!["hello", "world"]);
/// ```
pub fn promote_confident_tokens(
    stable: &mut Vec<String>,
    tokens: &[String],
    probs: &[f32],
    threshold: f32,
) -> usize {
    let mut added = 0;
    for (tok, &p) in tokens.iter().zip(probs.iter()).skip(stable.len()) {
        if p >= threshold {
            stable.push(tok.clone());
            added += 1;
        } else {
            break;
        }
    }
    added
}

fn diff_tokens(
    stable: &mut Vec<String>,
    last: &mut Vec<String>,
    new_tokens: &[String],
) -> (usize, Option<String>) {
    let prefix_len = last
        .iter()
        .zip(new_tokens)
        .take_while(|(a, b)| a == b)
        .count();
    let stable_len = stable.len();
    let mut new_stable = 0;
    if prefix_len > stable_len {
        stable.extend_from_slice(&new_tokens[stable_len..prefix_len]);
        new_stable = prefix_len - stable_len;
    }
    *last = new_tokens.to_vec();
    let fuzzy = if new_tokens.len() > prefix_len {
        Some(new_tokens[prefix_len..].join(" "))
    } else {
        None
    };
    (new_stable, fuzzy)
}

impl WhisperRecognizer {
    /// Load the Whisper model at `model_path`.
    pub fn new(model_path: &str) -> anyhow::Result<Arc<Self>> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        let recognizer = Arc::new(WhisperRecognizer {
            ctx,
            buffer: Mutex::new(Vec::new()),
            stable_tokens: Mutex::new(Vec::new()),
            last_tokens: Mutex::new(Vec::new()),
            last_result: Mutex::new(None),
            job: AsyncMutex::new(None),
            self_ref: Mutex::new(Weak::new()),
        });
        *recognizer.self_ref.lock().unwrap() = Arc::downgrade(&recognizer);
        Ok(recognizer)
    }

    /// Attempt to transcribe the buffered audio.
    pub fn transcribe(&self) -> anyhow::Result<Option<TranscriptResult>> {
        let buf_snapshot = {
            let buf = self.buffer.lock().unwrap();
            if buf.is_empty() {
                return Ok(None);
            }
            buf.clone()
        };

        tracing::debug!(
            samples = buf_snapshot.len(),
            ms = (buf_snapshot.len() as f32 / SAMPLE_RATE as f32) * 1000.0,
            "transcribe start"
        );
        if buf_snapshot.len() < MIN_SAMPLES {
            tracing::debug!("skipping transcription, not enough samples");
            return Ok(None);
        }

        let mut float_buf = vec![0.0f32; buf_snapshot.len()];
        whisper_rs::convert_integer_to_float_audio(&buf_snapshot, &mut float_buf)
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut state = self.ctx.create_state().map_err(|e| anyhow::anyhow!(e))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(num_cpus::get() as i32);
        state
            .full(params, &float_buf)
            .map_err(|e| anyhow::anyhow!(e))?;
        let segments = state.full_n_segments().map_err(|e| anyhow::anyhow!(e))?;
        let mut new_tokens = Vec::new();
        let mut probs = Vec::new();
        for i in 0..segments {
            let n_tokens = state.full_n_tokens(i).map_err(|e| anyhow::anyhow!(e))?;
            for t in 0..n_tokens {
                let text = state
                    .full_get_token_text(i, t)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let p = state
                    .full_get_token_prob(i, t)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    new_tokens.push(trimmed.to_string());
                    probs.push(p);
                }
            }
        }

        let mut stable = self.stable_tokens.lock().unwrap();
        let mut last = self.last_tokens.lock().unwrap();

        let (mut new_stable, _) = diff_tokens(&mut stable, &mut last, &new_tokens);
        new_stable +=
            promote_confident_tokens(&mut stable, &new_tokens, &probs, STABLE_PROB_THRESHOLD);

        if new_stable > 0 {
            let remove = buf_snapshot.len() * new_stable / new_tokens.len().max(1);
            let mut buf = self.buffer.lock().unwrap();
            let n = remove.min(buf.len());
            buf.drain(0..n);
            tracing::info!(
                "stable_emitted = {}",
                stable[stable.len() - new_stable..].join(" ")
            );
        }

        let fuzzy_tokens = if new_tokens.len() > stable.len() {
            Some(new_tokens[stable.len()..].join(" "))
        } else {
            None
        };

        tracing::debug!(
            fuzzy_tokens = fuzzy_tokens
                .as_ref()
                .map(|f| f.split_whitespace().count())
                .unwrap_or(0),
            buffer_ms = (buf_snapshot.len() as f32 / SAMPLE_RATE as f32) * 1000.0,
        );

        let result = TranscriptResult {
            stable: stable.join(" "),
            fuzzy: fuzzy_tokens,
        };
        *self.last_result.lock().unwrap() = Some(result.clone());

        Ok(Some(result))
    }

    async fn spawn_job(&self) {
        let weak = { self.self_ref.lock().unwrap().clone() };
        if let Some(this) = weak.upgrade() {
            let mut job = self.job.lock().await;
            if let Some(j) = job.as_ref() {
                if !j.handle.is_finished() {
                    tracing::trace!("transcription job already scheduled");
                    return;
                }
            }
            if let Some(j) = job.take() {
                j.token.cancel();
            }
            let token = CancellationToken::new();
            let cloned = this.clone();
            let tok = token.clone();
            let handle = tokio::spawn(async move {
                tokio::select! {
                    _ = tok.cancelled() => return,
                    _ = sleep(Duration::from_millis(DEBOUNCE_MS)) => {}
                }
                if tok.is_cancelled() {
                    return;
                }
                let _ = cloned.run_transcription(tok).await;
            });
            *job = Some(Job { token, handle });
        }
    }

    async fn run_transcription(self: Arc<Self>, token: CancellationToken) -> anyhow::Result<()> {
        if token.is_cancelled() {
            return Ok(());
        }
        tracing::debug!("running transcription task");
        let this = self.clone();
        let res = tokio::task::spawn_blocking(move || this.transcribe()).await?;
        if token.is_cancelled() {
            return Ok(());
        }
        if let Some(tr) = res? {
            *self.last_result.lock().unwrap() = Some(tr);
        }
        Ok(())
    }
}

#[async_trait]
impl SpeechRecognizer for WhisperRecognizer {
    async fn recognize(&self, samples: &[i16]) -> anyhow::Result<()> {
        let buf_len = {
            let mut buf = self.buffer.lock().unwrap();
            buf.extend_from_slice(samples);
            if buf.len() > MAX_SAMPLES {
                let excess = buf.len() - MAX_SAMPLES;
                buf.drain(0..excess);
            }
            buf.len()
        };
        tracing::debug!(
            samples = buf_len,
            ms = (buf_len as f32 / SAMPLE_RATE as f32) * 1000.0,
            "buffered"
        );
        self.spawn_job().await;
        Ok(())
    }

    async fn try_transcribe(&self) -> anyhow::Result<Option<TranscriptResult>> {
        Ok(self.last_result.lock().unwrap().take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_model_path_fails() {
        assert!(WhisperRecognizer::new("/no/model/here").is_err());
    }

    #[test]
    fn diff_tokens_tracks_stable_prefix() {
        let mut stable = vec!["hello".into()];
        let mut last = vec!["hello".into(), "world".into()];
        let new = vec!["hello".into(), "there".into()];
        let (added, fuzzy) = diff_tokens(&mut stable, &mut last, &new);
        assert_eq!(added, 0);
        assert_eq!(stable, vec!["hello".to_string()]);
        assert_eq!(fuzzy, Some("there".into()));
    }

    #[test]
    fn diff_tokens_emits_new_stable() {
        let mut stable = Vec::new();
        let mut last = Vec::new();
        let new = vec!["one".into(), "two".into()];
        let (added, fuzzy) = diff_tokens(&mut stable, &mut last, &new);
        assert_eq!(added, 0);
        assert_eq!(stable, Vec::<String>::new());
        assert_eq!(fuzzy, Some("one two".into()));
    }

    #[test]
    fn promotes_tokens_by_confidence() {
        let mut stable = vec!["hello".to_string()];
        let tokens = vec!["hello".to_string(), "world".to_string(), "foo".to_string()];
        let probs = vec![0.99, 0.9, 0.4];
        let added = promote_confident_tokens(&mut stable, &tokens, &probs, 0.85);
        assert_eq!(added, 1);
        assert_eq!(stable, vec!["hello", "world"]);
    }
}
