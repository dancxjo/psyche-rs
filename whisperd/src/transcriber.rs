use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{DateTime, Local};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error};

use crate::{Stt, send_transcription};

pub struct SegmentJob {
    pub id: u64,
    pub pcm: Vec<i16>,
    pub when: DateTime<Local>,
    /// When the segment was received from the socket.
    pub received: DateTime<Local>,
    pub writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
}

pub fn spawn_transcriber(
    mut rx: mpsc::Receiver<SegmentJob>,
    stt: Arc<dyn Stt + Send + Sync>,
    latest: Arc<AtomicU64>,
) {
    tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            if job.id < latest.load(Ordering::Relaxed) {
                debug!(id = job.id, "Skipping obsolete transcription job");
                continue;
            }
            let handle = tokio::runtime::Handle::current();
            let res = tokio::task::spawn_blocking({
                let stt = stt.clone();
                let pcm = job.pcm.clone();
                move || handle.block_on(stt.transcribe(&pcm))
            })
            .await;

            match res {
                Ok(Ok(mut trans)) => {
                    trans.when = job.when;
                    latest.store(job.id, Ordering::Relaxed);
                    let mut w = job.writer.lock().await;
                    if let Err(e) = send_transcription(&mut *w, &trans).await {
                        error!(?e, "failed to send transcription");
                    }
                }
                Ok(Err(e)) => {
                    error!(?e, "transcription failed");
                }
                Err(e) => {
                    error!(?e, "transcription panicked");
                }
            }
        }
    });
}
