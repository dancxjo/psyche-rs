use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use stream_prefix::parse_timestamp_prefix;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, trace};

const SEGMENTS_ENV: &str = "WHISPER_SEGMENTS_DIR";
/// Embedded default systemd unit.
const SYSTEMD_UNIT: &str = include_str!("../whisperd.service");

/// Return the default systemd unit file for `whisperd`.
///
/// ```
/// use whisperd::systemd_unit;
/// assert!(systemd_unit().contains("Whisper Audio"));
/// ```
pub fn systemd_unit() -> &'static str {
    SYSTEMD_UNIT
}
/// (De)serialize `DateTime<Local>` as seconds since the Unix epoch.
mod local_ts_seconds {
    use super::*;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(dt: &DateTime<Local>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_i64(dt.timestamp())
    }

    pub fn deserialize<'de, D>(de: D) -> Result<DateTime<Local>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ts = i64::deserialize(de)?;
        let ndt = NaiveDateTime::from_timestamp_opt(ts, 0)
            .ok_or_else(|| serde::de::Error::custom("invalid timestamp"))?;
        Ok(DateTime::<Utc>::from_utc(ndt, Utc).with_timezone(&Local))
    }
}
mod audio_segmenter;
use audio_segmenter::AudioSegmenter;
mod transcriber;
use transcriber::{SegmentJob, spawn_transcriber};
pub mod model;

#[cfg(test)]
pub mod test_helpers;

/// Result of a transcription.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Transcription {
    /// Combined text output.
    pub text: String,
    /// Word-level timing information.
    pub words: Vec<Word>,
    /// When the recorded audio began.
    #[serde(with = "local_ts_seconds")]
    pub when: chrono::DateTime<chrono::Local>,
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
            let mut lock = words_cb.blocking_lock();
            lock.push(seg);
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
            // TODO: use the actual start time of the audio
            when: chrono::Local::now(),
        })
    }
}

/// Run the daemon.
pub async fn run(
    socket: PathBuf,
    model: PathBuf,
    silence_ms: u64,
    timeout_ms: u64,
    max_queue: usize,
) -> anyhow::Result<()> {
    info!(?socket, "starting whisperd");
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, "listening for PCM input");

    let stt = std::sync::Arc::new(WhisperStt::new(model)?);
    let (tx, rx) = mpsc::channel::<SegmentJob>(max_queue);
    let latest_processed_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    spawn_transcriber(rx, stt.clone(), latest_processed_id.clone());

    let segment_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    loop {
        let (stream, _) = listener.accept().await?;
        let tx = tx.clone();
        let counter = segment_counter.clone();
        if let Err(e) = handle_connection(stream, tx, counter, silence_ms, timeout_ms).await {
            error!(?e, "connection error");
        }
    }
}

pub(crate) async fn handle_connection(
    stream: UnixStream,
    tx: mpsc::Sender<SegmentJob>,
    segment_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    silence_ms: u64,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let (mut reader, writer) = stream.into_split();
    let writer = std::sync::Arc::new(Mutex::new(writer));
    let mut buf = [0u8; 4096];
    let samples_per_ms = audio_segmenter::FRAME_SIZE / 30;
    let silence_samples = (silence_ms as usize * samples_per_ms).max(audio_segmenter::FRAME_SIZE);
    let timeout_samples = (timeout_ms as usize * samples_per_ms).max(audio_segmenter::FRAME_SIZE);
    let mut segmenter = AudioSegmenter::with_timeout(silence_samples, timeout_samples);
    const SAMPLE_RATE_HZ: i64 = 16_000;
    let mut last_discard = 0usize;
    let mut current_when: DateTime<Local> = chrono::Local::now();
    let mut first_chunk = true;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let mut start = 0usize;
        if first_chunk {
            if let Some((ts, idx)) = parse_timestamp_prefix(&buf[..n]) {
                current_when = ts;
                start = idx;
                if start < n && buf[start] == b'\n' {
                    start += 1;
                }
                trace!(when=%current_when, "timestamp received");
            }
            first_chunk = false;
        }
        let mut frames = Vec::with_capacity((n - start) / 2);
        for chunk in buf[start..n].chunks_exact(2) {
            frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        trace!(frames = frames.len(), "received audio frames");
        while let Some(spoken) = segmenter.push_frames(&frames) {
            let when = current_when;
            let len = spoken.len();
            queue_transcription(spoken, when, &writer, &tx, &segment_counter);
            current_when = current_when
                + chrono::Duration::microseconds(len as i64 * 1_000_000 / SAMPLE_RATE_HZ);
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
        queue_transcription(spoken, current_when, &writer, &tx, &segment_counter);
    }

    Ok(())
}

pub(crate) fn queue_transcription(
    spoken: Vec<i16>,
    when: DateTime<Local>,
    writer: &std::sync::Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    tx: &mpsc::Sender<SegmentJob>,
    counter: &std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    let id = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let job = SegmentJob {
        id,
        pcm: spoken,
        when,
        writer: writer.clone(),
    };
    if let Err(_) = tx.try_send(job) {
        debug!("Transcription dropped due to full queue");
    }
}

pub(crate) async fn send_transcription<W>(
    stream: &mut W,
    result: &Transcription,
) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    stream
        .write_all(format!("@{{{}}} {}\n", result.when.to_rfc3339(), result.text).as_bytes())
        .await?;
    debug!(text = %result.text, when = %result.when, "sent transcription");
    Ok(())
}

pub(crate) async fn save_segment(pcm: &[i16]) -> anyhow::Result<()> {
    let dir = match std::env::var(SEGMENTS_ENV) {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let dir = std::path::PathBuf::from(dir);
    tokio::fs::create_dir_all(&dir).await.ok();
    let ts = chrono::Utc::now().timestamp_millis();
    let path = dir.join(format!("segment_{ts}.wav"));
    let pcm = pcm.to_vec();
    let p = path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&p, spec)?;
        for sample in pcm {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        Ok(())
    })
    .await??;
    debug!(?path, "segment saved");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::run_with_stt_no_vad;
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
                when: Local::now().into(),
                raw: None,
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transcribes_audio_on_same_socket() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("ear.sock");

        let stt = MockStt;
        let local = tokio::task::LocalSet::new();
        let sock_clone = sock.clone();
        let handle = local.spawn_local(async move {
            run_with_stt_no_vad(sock_clone, stt, 1000, 20000, 16)
                .await
                .unwrap();
        });

        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut s = UnixStream::connect(&sock).await.unwrap();
                let mut samples: Vec<i16> = vec![1000; 16000];
                samples.extend(vec![0; 17000]);
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                s.write_all(&bytes).await.unwrap();
                tokio::io::AsyncWriteExt::shutdown(&mut s).await.unwrap();
                let mut buf = String::new();
                s.read_to_string(&mut buf).await.unwrap();
                assert!(buf.starts_with("@{"));
                assert!(buf.contains("hello"));
            })
            .await;
        handle.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn saves_segments_when_env_set() {
        let seg_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(SEGMENTS_ENV, seg_dir.path());
        }

        let samples: Vec<i16> = vec![1000; 16000];
        save_segment(&samples).await.unwrap();

        let count = std::fs::read_dir(seg_dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .map(|ext| ext == "wav")
                    .unwrap_or(false)
            })
            .count();
        unsafe {
            std::env::remove_var(SEGMENTS_ENV);
        }
        assert_eq!(count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_transcription_writes_to_stream() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let result = Transcription {
            text: "test".into(),
            words: vec![],
            when: Local::now().into(),
            raw: None,
        };
        let recv = tokio::spawn(async move {
            let mut buf = String::new();
            b.read_to_string(&mut buf).await.unwrap();
            buf
        });
        send_transcription(&mut a, &result).await.unwrap();
        tokio::io::AsyncWriteExt::shutdown(&mut a).await.unwrap();
        let received = recv.await.unwrap();
        assert!(received.starts_with("@{"));
        assert!(received.contains("test"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn emits_timestamp_from_pcm_prefix() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("ear.sock");

        let stt = MockStt;
        let local = tokio::task::LocalSet::new();
        let sock_clone = sock.clone();
        let handle = local.spawn_local(async move {
            run_with_stt_no_vad(sock_clone, stt, 1000, 20000, 16)
                .await
                .unwrap();
        });

        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut s = UnixStream::connect(&sock).await.unwrap();
                let ts = Local::now();
                let mut bytes = format!("@{{{}}}", ts.to_rfc3339()).into_bytes();
                let samples: Vec<i16> = vec![1000; 16000];
                bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
                s.write_all(&bytes).await.unwrap();
                tokio::io::AsyncWriteExt::shutdown(&mut s).await.unwrap();
                let mut buf = String::new();
                s.read_to_string(&mut buf).await.unwrap();
                let end = buf.find('}').unwrap();
                let out_ts = &buf[2..end];
                let out_dt = chrono::DateTime::parse_from_rfc3339(out_ts).unwrap();
                let delta = (out_dt.with_timezone(&Local) - ts).num_milliseconds().abs();
                assert!(delta < 10);
            })
            .await;
        handle.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drops_segments_when_queue_full() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct SlowStt {
            calls: std::sync::Arc<AtomicUsize>,
        };

        #[async_trait]
        impl Stt for SlowStt {
            async fn transcribe(&self, _pcm: &[i16]) -> anyhow::Result<Transcription> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                Ok(Transcription {
                    text: "ok".into(),
                    words: vec![],
                    when: Local::now(),
                    raw: None,
                })
            }
        }

        let dir = tempdir().unwrap();
        let sock = dir.path().join("ear.sock");
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let stt = SlowStt {
            calls: calls.clone(),
        };
        let local = tokio::task::LocalSet::new();
        let sock_clone = sock.clone();
        let handle = local.spawn_local(async move {
            run_with_stt_no_vad(sock_clone, stt, 1000, 20000, 1)
                .await
                .unwrap();
        });

        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut s = UnixStream::connect(&sock).await.unwrap();
                let mut samples: Vec<i16> = Vec::new();
                for _ in 0..3 {
                    samples.extend(vec![1000i16; 16000]);
                    samples.extend(vec![0i16; 17000]);
                }
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                s.write_all(&bytes).await.unwrap();
                tokio::io::AsyncWriteExt::shutdown(&mut s).await.unwrap();
                let mut buf = String::new();
                s.read_to_string(&mut buf).await.unwrap();
            })
            .await;
        handle.abort();
        assert!(calls.load(Ordering::SeqCst) < 3);
    }
}
