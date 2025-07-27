use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify, broadcast};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

#[derive(Clone)]
struct TextQueue {
    inner: Arc<Mutex<VecDeque<String>>>,
    notify: Arc<Notify>,
}

impl TextQueue {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    async fn push(&self, text: String) {
        let mut q = self.inner.lock().await;
        q.push_back(text);
        self.notify.notify_one();
    }

    async fn pop(&self) -> Option<String> {
        let mut q = self.inner.lock().await;
        q.pop_front()
    }

    async fn clear(&self) {
        let mut q = self.inner.lock().await;
        q.clear();
    }

    async fn wait(&self) {
        self.notify.notified().await;
    }
}

#[derive(Clone)]
enum PcmMessage {
    Data(Vec<u8>),
    Stop,
}

async fn synth_loop(
    tts_url: String,
    speaker_id: String,
    language_id: String,
    queue: TextQueue,
    tx: broadcast::Sender<PcmMessage>,
    cancel: Arc<Mutex<CancellationToken>>,
) {
    loop {
        let text = loop {
            if let Some(t) = queue.pop().await {
                break t;
            }
            queue.wait().await;
        };
        let token = { cancel.lock().await.clone() };
        let fut = synthesize(&tts_url, &speaker_id, &language_id, &text);
        let res = tokio::select! {
            r = fut => r.ok(),
            _ = token.cancelled() => None,
        };
        if let Some(pcm) = res {
            if !token.is_cancelled() {
                let _ = tx.send(PcmMessage::Data(pcm));
            }
        }
    }
}

/// Extracts the raw PCM payload from a WAV file.
fn wav_to_pcm(bytes: &[u8]) -> anyhow::Result<&[u8]> {
    // very naive: assume 44 byte header
    if bytes.len() <= 44 {
        anyhow::bail!("wav data too short");
    }
    Ok(&bytes[44..])
}

async fn synthesize(
    tts_url: &str,
    speaker_id: &str,
    language_id: &str,
    text: &str,
) -> anyhow::Result<Vec<u8>> {
    let url = format!(
        "{}/api/tts?text={}&speaker_id={}&style_wav=&language_id={}",
        tts_url,
        urlencoding::encode(text),
        speaker_id,
        language_id
    );
    let resp = reqwest::get(&url).await?.error_for_status()?;
    let wav = resp.bytes().await?;
    Ok(wav_to_pcm(&wav)?.to_vec())
}

async fn handle_connection(
    stream: UnixStream,
    queue: TextQueue,
    pcm_tx: broadcast::Sender<PcmMessage>,
    cancel: Arc<Mutex<CancellationToken>>,
) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut rx = pcm_tx.subscribe();
    let mut buf = Vec::new();
    loop {
        tokio::select! {
            read = reader.read_until(b'\n', &mut buf) => {
                let n = read?;
                if n == 0 { break; }
                if let Some(pos) = buf.iter().position(|b| *b == 0x03) {
                    buf.drain(..=pos);
                    queue.clear().await;
                    {
                        let mut c = cancel.lock().await;
                        c.cancel();
                        *c = CancellationToken::new();
                    }
                    let _ = pcm_tx.send(PcmMessage::Stop);
                }
                if !buf.is_empty() {
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        let text = text.trim();
                        if !text.is_empty() {
                            queue.push(text.to_string()).await;
                        }
                    }
                }
                buf.clear();
            }
            msg = rx.recv() => match msg {
                Ok(PcmMessage::Data(pcm)) => {
                    if let Err(e) = write_half.write_all(&pcm).await {
                        error!(?e, "write failed");
                        break;
                    }
                }
                Ok(PcmMessage::Stop) => {
                    write_half.shutdown().await.ok();
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            },
        }
    }
    Ok(())
}

/// Run the spoken daemon.
pub async fn run(
    socket: PathBuf,
    tts_url: String,
    speaker_id: String,
    language_id: String,
) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, ?tts_url, %speaker_id, %language_id, "spoken listening");

    let queue = TextQueue::new();
    let cancel = Arc::new(Mutex::new(CancellationToken::new()));
    let (pcm_tx, _) = broadcast::channel(8);

    tokio::spawn(synth_loop(
        tts_url.clone(),
        speaker_id.clone(),
        language_id.clone(),
        queue.clone(),
        pcm_tx.clone(),
        cancel.clone(),
    ));

    loop {
        let (stream, _) = listener.accept().await?;
        let q = queue.clone();
        let tx = pcm_tx.clone();
        let c = cancel.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, q, tx, c).await {
                error!(?e, "connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use tempfile::tempdir;
    use tokio::net::UnixStream;
    use tokio::task::LocalSet;

    fn wav_bytes() -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample::<i16>(0).unwrap();
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    #[tokio::test]
    async fn synthesizes_input_line() {
        let server = MockServer::start_async().await;
        let wav = wav_bytes();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/api/tts");
            then.status(200).body(wav.clone());
        });
        let dir = tempdir().unwrap();
        let sock = dir.path().join("voice.sock");
        let url = format!("http://{}", server.address());
        let local = LocalSet::new();
        let handle = local.spawn_local(run(sock.clone(), url, "p330".into(), "".into()));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut client = UnixStream::connect(&sock).await.unwrap();
                tokio::io::AsyncWriteExt::write_all(&mut client, b"hello\n")
                    .await
                    .unwrap();
                let mut buf = [0u8; 8];
                let n = tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    tokio::io::AsyncReadExt::read(&mut client, &mut buf),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(n > 0);
            })
            .await;
        handle.abort();
    }
}
