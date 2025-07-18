use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, trace};
mod audio_segmenter;
use audio_segmenter::AudioSegmenter;

#[cfg(test)]
pub mod test_helpers;

/// Result of a transcription.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Transcription {
    /// Combined text output.
    pub text: String,
    /// Word-level timing information.
    pub words: Vec<Word>,
    /// Optional raw whisper result serialized to JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// Word timing data.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Word {
    pub word: String,
    pub start: f32,
    pub end: f32,
}

#[async_trait]
pub trait Stt {
    async fn transcribe(&self, pcm: &[i16]) -> anyhow::Result<Transcription>;
}

/// Wrapper around whisper-rs providing [`Stt`].
pub struct WhisperStt {
    ctx: whisper_rs::WhisperContext,
    params: whisper_rs::FullParams<'static, 'static>,
}

impl WhisperStt {
    /// Load a whisper model from the given path.
    pub fn new(model: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let path = model
            .as_ref()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path must be valid UTF-8"))?;
        let ctx = whisper_rs::WhisperContext::new_with_params(
            path,
            whisper_rs::WhisperContextParameters::default(),
        )?;
        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::default());
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(true);
        Ok(Self { ctx, params })
    }
}

#[async_trait]
impl Stt for WhisperStt {
    async fn transcribe(&self, pcm: &[i16]) -> anyhow::Result<Transcription> {
        let pcm_f32: Vec<f32> = pcm.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
        let mut state = self.ctx.create_state()?;
        let mut params = self.params.clone();
        let words = std::sync::Arc::new(Mutex::new(Vec::new()));
        let words_cb = words.clone();
        params.set_segment_callback_safe_lossy(move |seg| {
            trace!(?seg, "segment");
            words_cb.blocking_lock().push(seg);
        });
        tokio::task::spawn_blocking(move || state.full(params, &pcm_f32)).await??;
        let segs = words.lock().await.clone();
        let mut text = String::new();
        let mut out_words = Vec::new();
        for seg in &segs {
            text.push_str(&seg.text);
            out_words.push(Word {
                word: seg.text.clone(),
                start: seg.start_timestamp as f32 / 100.0,
                end: seg.end_timestamp as f32 / 100.0,
            });
        }
        let include_raw = std::env::var("WHISPER_INCLUDE_RAW")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let raw = if include_raw {
            let raw_segments: Vec<_> = segs
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "text": s.text,
                        "start": s.start_timestamp,
                        "end": s.end_timestamp,
                    })
                })
                .collect();
            Some(serde_json::json!({ "segments": raw_segments }))
        } else {
            None
        };
        Ok(Transcription {
            text: text.trim().to_string(),
            words: out_words,
            raw,
        })
    }
}

/// Run the daemon.
pub async fn run(socket: PathBuf, listen: PathBuf, model: PathBuf) -> anyhow::Result<()> {
    info!(?socket, ?listen, "starting heard");
    if listen.exists() {
        tokio::fs::remove_file(&listen).await.ok();
    }
    let listener = UnixListener::bind(&listen)?;
    info!(?listen, "listening for PCM input");

    let stt = WhisperStt::new(model)?;

    let (tx, mut rx) = mpsc::channel::<Vec<i16>>(8);

    // Accept loop
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut stream, _addr)) => {
                    debug!("accepted connection");
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        loop {
                            match stream.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    let mut frames = Vec::with_capacity(n / 2);
                                    for chunk in buf[..n].chunks_exact(2) {
                                        frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                                    }
                                    trace!(frames = frames.len(), "received audio frames");
                                    if tx.send(frames).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    error!(?e, "read error");
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    error!(?e, "accept failed");
                    break;
                }
            }
        }
    });

    let mut segmenter = AudioSegmenter::new();
    let mut last_discard = 0usize;
    while let Some(chunk) = rx.recv().await {
        if let Some(spoken) = segmenter.push_frames(&chunk) {
            debug!(samples = spoken.len(), "transcribing audio");
            if let Ok(trans) = stt.transcribe(&spoken).await {
                debug!(text = %trans.text, "transcription done");
                send_transcription(&socket, &trans).await.ok();
            }
        }
        if segmenter.discarded() != last_discard {
            debug!(
                discarded = segmenter.discarded() - last_discard,
                "discarded short audio"
            );
            last_discard = segmenter.discarded();
        }
    }

    if let Some(spoken) = segmenter.finish() {
        debug!(samples = spoken.len(), "transcribing audio");
        if let Ok(trans) = stt.transcribe(&spoken).await {
            debug!(text = %trans.text, "transcription done");
            send_transcription(&socket, &trans).await.ok();
        }
    }

    Ok(())
}

pub(crate) async fn send_transcription(
    socket: &PathBuf,
    result: &Transcription,
) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket).await?;
    let text = serde_json::to_string(result)?;
    stream
        .write_all(format!("/heard/asr\n{}\n---\n", text).as_bytes())
        .await?;
    debug!(text = %result.text, "sent transcription");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::run_with_stt;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    struct MockStt;

    #[async_trait]
    impl Stt for MockStt {
        async fn transcribe(&self, _pcm: &[i16]) -> anyhow::Result<Transcription> {
            Ok(Transcription {
                text: "hello".into(),
                words: vec![Word {
                    word: "hello".into(),
                    start: 0.0,
                    end: 0.5,
                }],
                raw: None,
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore]
    async fn sends_transcription_over_socket() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("out.sock");
        let listen = dir.path().join("in.sock");

        // Create listener for output to capture messages
        let listener = UnixListener::bind(&out).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = String::new();
            stream.read_to_string(&mut buf).await.unwrap();
            buf
        });

        // Run daemon with mocked STT
        let stt = MockStt;
        let local = tokio::task::LocalSet::new();
        let out_clone = out.clone();
        let listen_clone = listen.clone();
        let handle = local.spawn_local(async move {
            run_with_stt(out_clone, listen_clone, stt).await.unwrap();
        });

        let listen_client = listen.clone();

        local
            .run_until(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut s = UnixStream::connect(&listen_client).await.unwrap();
                let mut samples: Vec<i16> = vec![1000; 16000];
                samples.extend(vec![0; 16000]);
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                s.write_all(&bytes).await.unwrap();
                drop(s);

                let received = server.await.unwrap();
                assert!(received.contains("/heard/asr"));
                assert!(received.contains("\"hello\""));
            })
            .await;
        handle.abort();
    }
}
