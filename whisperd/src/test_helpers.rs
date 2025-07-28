use std::path::PathBuf;

use crate::{Stt, handle_connection};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tracing::error;

/// Helper to run the whisperd loop with a custom STT implementation.
pub async fn run_with_stt<S: Stt + Send + Sync + 'static>(
    socket: PathBuf,
    stt: S,
    silence_ms: u64,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    let stt = std::sync::Arc::new(stt);
    loop {
        let (stream, _) = listener.accept().await?;
        let stt = stt.clone();
        if let Err(e) = handle_connection(stream, stt, silence_ms).await {
            error!(?e, "connection error");
        }
    }
}

/// Variant of [`run_with_stt`] without VAD for deterministic testing.
pub async fn run_with_stt_no_vad<S: Stt + Send + Sync + 'static>(
    socket: PathBuf,
    stt: S,
    silence_ms: u64,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    let stt = std::sync::Arc::new(stt);
    loop {
        let (mut stream, _) = listener.accept().await?;
        let stt = stt.clone();
        let mut buf = [0u8; 4096];
        let silence_samples = (silence_ms as usize * crate::audio_segmenter::FRAME_SIZE / 30)
            .max(crate::audio_segmenter::FRAME_SIZE);
        let mut segmenter =
            crate::audio_segmenter::AudioSegmenter::new_without_vad(silence_samples);
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let mut frames = Vec::with_capacity(n / 2);
            for chunk in buf[..n].chunks_exact(2) {
                frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
            if let Some(spoken) = segmenter.push_frames(&frames) {
                if let Ok(trans) = stt.transcribe(&spoken).await {
                    crate::send_transcription(&mut stream, &trans).await?;
                }
            }
        }
        if let Some(spoken) = segmenter.finish() {
            if let Ok(trans) = stt.transcribe(&spoken).await {
                crate::send_transcription(&mut stream, &trans).await?;
            }
        }
    }
}
