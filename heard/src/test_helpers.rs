use std::path::PathBuf;

use crate::{Stt, audio_segmenter::AudioSegmenter, send_transcription};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::debug;

/// Helper to run the heard loop with a custom STT implementation.
pub async fn run_with_stt<S: Stt + 'static>(
    socket: PathBuf,
    listen: PathBuf,
    stt: S,
) -> anyhow::Result<()> {
    if listen.exists() {
        tokio::fs::remove_file(&listen).await.ok();
    }
    let listener = UnixListener::bind(&listen)?;
    let (tx, mut rx) = mpsc::channel::<Vec<i16>>(8);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    let mut frames = Vec::with_capacity(n / 2);
                    for chunk in buf[..n].chunks_exact(2) {
                        frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                    }
                    tx.send(frames).await.unwrap();
                }
            });
        }
    });
    let mut seg = AudioSegmenter::new();
    while let Some(chunk) = rx.recv().await {
        if let Some(spoken) = seg.push_frames(&chunk) {
            if let Ok(trans) = stt.transcribe(&spoken).await {
                send_transcription(&socket, &trans).await.unwrap();
            }
        }
    }
    if let Some(spoken) = seg.finish() {
        if let Ok(trans) = stt.transcribe(&spoken).await {
            send_transcription(&socket, &trans).await.unwrap();
        }
    }
    debug!(discarded = seg.discarded(), "total discarded frames");
    Ok(())
}
