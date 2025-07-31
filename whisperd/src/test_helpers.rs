use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::{
    Stt, handle_connection,
    transcriber::{SegmentJob, spawn_transcriber},
};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::error;

/// Helper to run the whisperd loop with a custom STT implementation.
pub async fn run_with_stt<S: Stt + Send + Sync + 'static>(
    socket: PathBuf,
    stt: S,
    silence_ms: u64,
    timeout_ms: u64,
    max_queue: usize,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    let stt = Arc::new(stt);
    let (tx, rx) = mpsc::channel::<SegmentJob>(max_queue);
    let latest = Arc::new(AtomicU64::new(0));
    spawn_transcriber(rx, stt, latest);
    let counter = Arc::new(AtomicU64::new(0));
    loop {
        let (stream, _) = listener.accept().await?;
        let tx = tx.clone();
        let counter = counter.clone();
        tokio::task::spawn_local(async move {
            if let Err(e) = handle_connection(stream, tx, counter, silence_ms, timeout_ms).await {
                error!(?e, "connection error");
            }
        });
    }
}

/// Variant of [`run_with_stt`] without VAD for deterministic testing.
pub async fn run_with_stt_no_vad<S: Stt + Send + Sync + 'static>(
    socket: PathBuf,
    stt: S,
    silence_ms: u64,
    timeout_ms: u64,
    max_queue: usize,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    let stt = Arc::new(stt);
    let (tx, rx) = mpsc::channel::<SegmentJob>(max_queue);
    let latest = Arc::new(AtomicU64::new(0));
    spawn_transcriber(rx, stt.clone(), latest);
    let counter = Arc::new(AtomicU64::new(0));
    loop {
        let (stream, _) = listener.accept().await?;
        let tx = tx.clone();
        let counter = counter.clone();
        let stt = stt.clone();
        let silence_samples = (silence_ms as usize * crate::audio_segmenter::FRAME_SIZE / 30)
            .max(crate::audio_segmenter::FRAME_SIZE);
        let timeout_samples = (timeout_ms as usize * crate::audio_segmenter::FRAME_SIZE / 30)
            .max(crate::audio_segmenter::FRAME_SIZE);
        tokio::task::spawn_local(async move {
            let (mut reader, writer) = stream.into_split();
            let writer = Arc::new(tokio::sync::Mutex::new(writer));
            let mut buf = [0u8; 4096];
            let mut segmenter = crate::audio_segmenter::AudioSegmenter::without_vad_with_timeout(
                silence_samples,
                timeout_samples,
            );
            loop {
                let n = reader.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                let mut frames = Vec::with_capacity(n / 2);
                for chunk in buf[..n].chunks_exact(2) {
                    frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                }
                if let Some(spoken) = segmenter.push_frames(&frames) {
                    crate::save_segment(&spoken).await.ok();
                    crate::queue_transcription(
                        spoken,
                        chrono::Local::now(),
                        &writer,
                        &tx,
                        &counter,
                    );
                }
            }
            if let Some(spoken) = segmenter.finish() {
                crate::queue_transcription(spoken, chrono::Local::now(), &writer, &tx, &counter);
            }
        });
    }
}
