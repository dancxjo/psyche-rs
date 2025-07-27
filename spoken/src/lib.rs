use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info};

/// Extracts the raw PCM payload from a WAV file.
fn wav_to_pcm(bytes: &[u8]) -> anyhow::Result<&[u8]> {
    // very naive: assume 44 byte header
    if bytes.len() <= 44 {
        anyhow::bail!("wav data too short");
    }
    Ok(&bytes[44..])
}

async fn synthesize(tts_url: &str, text: &str) -> anyhow::Result<Vec<u8>> {
    let url = format!("{}/api/tts?text={}", tts_url, urlencoding::encode(text));
    let resp = reqwest::get(&url).await?.error_for_status()?;
    let wav = resp.bytes().await?;
    Ok(wav_to_pcm(&wav)?.to_vec())
}

async fn handle_connection(mut stream: UnixStream, tts_url: String) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        let text = line.trim();
        if !text.is_empty() {
            debug!(%text, "synthesizing");
            match synthesize(&tts_url, text).await {
                Ok(pcm) => reader.get_mut().write_all(&pcm).await?,
                Err(e) => error!(?e, "tts failed"),
            }
        }
        line.clear();
    }
    reader.into_inner().shutdown().await?;
    Ok(())
}

/// Run the spoken daemon.
pub async fn run(socket: PathBuf, tts_url: String) -> anyhow::Result<()> {
    if socket.exists() {
        tokio::fs::remove_file(&socket).await.ok();
    }
    let listener = UnixListener::bind(&socket)?;
    info!(?socket, ?tts_url, "spoken listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let url = tts_url.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, url).await {
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
        let handle = local.spawn_local(run(sock.clone(), url));
        local
            .run_until(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let mut client = UnixStream::connect(&sock).await.unwrap();
                tokio::io::AsyncWriteExt::write_all(&mut client, b"hello\n")
                    .await
                    .unwrap();
                client.shutdown().await.unwrap();
                let mut buf = Vec::new();
                tokio::io::AsyncReadExt::read_to_end(&mut client, &mut buf)
                    .await
                    .unwrap();
                assert!(!buf.is_empty());
            })
            .await;
        handle.abort();
    }
}
