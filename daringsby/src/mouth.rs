use bytes::Bytes;
use futures::StreamExt;
use hound::WavReader;
use segtok::segmenter::{SegmentConfig, split_single};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{trace, warn};

use psyche_rs::{Action, ActionResult, Motor, MotorError};

/// Motor that streams text-to-speech audio via HTTP.
///
/// Sentences from the input body are sent to a TTS service and the
/// resulting audio bytes are broadcast to subscribers.
///
/// # Example
/// ```no_run
/// use daringsby::Mouth;
/// let mouth = Mouth::default();
/// let _ = mouth.subscribe();
/// ```
pub struct Mouth {
    client: reqwest::Client,
    base_url: String,
    language_id: Option<String>,
    tx: Sender<Bytes>,
    queue: Arc<TokioMutex<()>>,
}

impl Default for Mouth {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(64);
        let queue = Arc::new(TokioMutex::new(()));
        Self {
            client: reqwest::Client::new(),
            base_url: "http://localhost:5002".into(),
            language_id: None,
            tx,
            queue,
        }
    }
}

impl Mouth {
    /// Creates a mouth with the given base URL and optional language.
    pub fn new(base_url: impl Into<String>, language_id: Option<String>) -> Self {
        let (tx, _) = broadcast::channel(64);
        let queue = Arc::new(TokioMutex::new(()));
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            language_id,
            tx,
            queue,
        }
    }

    /// Subscribes to the audio stream.
    pub fn subscribe(&self) -> Receiver<Bytes> {
        self.tx.subscribe()
    }

    /// Convert a WAV byte buffer to 16-bit PCM bytes.
    ///
    /// ```
    /// use daringsby::mouth::Mouth;
    /// # fn make_wav() -> Vec<u8> {
    /// #   let spec = hound::WavSpec {
    /// #       channels: 1,
    /// #       sample_rate: 22_050,
    /// #       bits_per_sample: 16,
    /// #       sample_format: hound::SampleFormat::Int,
    /// #   };
    /// #   let mut bytes = Vec::new();
    /// #   let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec).unwrap();
    /// #   w.write_sample(0i16).unwrap();
    /// #   w.write_sample(1i16).unwrap();
    /// #   w.finalize().unwrap();
    /// #   bytes
    /// # }
    /// let wav = make_wav();
    /// let pcm = Mouth::wav_to_pcm(&wav).unwrap();
    /// assert_eq!(pcm.len(), 4); // two i16 samples
    /// ```
    pub fn wav_to_pcm(data: &[u8]) -> Result<Bytes, MotorError> {
        let mut reader = WavReader::new(Cursor::new(data))
            .map_err(|e| MotorError::Failed(format!("wav decode failed: {e}")))?;
        let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
        let samples = samples.map_err(|e| MotorError::Failed(format!("wav read failed: {e}")))?;
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        Ok(Bytes::from(buf))
    }

    fn tts_url(
        base: &str,
        text: &str,
        speaker_id: &str,
        language: &str,
    ) -> Result<String, url::ParseError> {
        let mut url = reqwest::Url::parse(base)?.join("api/tts")?;
        url.query_pairs_mut()
            .append_pair(
                "text",
                &text.chars().filter(|c| *c != '*').collect::<String>(),
            )
            .append_pair("speaker_id", speaker_id)
            .append_pair("style_wav", "")
            .append_pair("language_id", language);
        trace!(url = %url, "TTS URL");
        Ok(url.into())
    }
}

#[async_trait::async_trait]
impl Motor for Mouth {
    fn description(&self) -> &'static str {
        "Streams TTS audio from text via HTTP"
    }

    fn name(&self) -> &'static str {
        "speak"
    }

    async fn perform(&self, mut action: Action) -> Result<ActionResult, MotorError> {
        if action.intention.urge.name != "speak" {
            return Err(MotorError::Unrecognized);
        }
        let speaker_id = action
            .intention
            .urge
            .args
            .get("speaker_id")
            .map(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some("p234".into()))
            .unwrap_or_default();
        let lang = action
            .intention
            .urge
            .args
            .get("language_id")
            .map(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.language_id.clone())
            .unwrap_or_default();
        let client = self.client.clone();
        let lang = lang.clone();
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        let queue = self.queue.clone();
        tokio::spawn(async move {
            let _guard = queue.lock().await;
            let mut buf = String::new();
            while let Some(chunk) = action.body.next().await {
                buf.push_str(&chunk);
                let mut sents = split_single(&buf, SegmentConfig::default());
                if let Some(last) = sents.last() {
                    if !last.trim_end().ends_with(['.', '!', '?']) {
                        buf = last.clone();
                        sents.pop();
                    } else {
                        buf.clear();
                    }
                }
                for sent in sents {
                    trace!(%sent, "tts sentence");
                    let url = match Mouth::tts_url(&base, &sent, &speaker_id, &lang) {
                        Ok(u) => u,
                        Err(e) => {
                            warn!(error=?e, "tts url error");
                            continue;
                        }
                    };
                    match client.get(url).send().await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(body) => match Mouth::wav_to_pcm(&body) {
                                Ok(pcm) => {
                                    let _ = tx.send(pcm);
                                }
                                Err(e) => warn!(error = ?e, "wav decode failed"),
                            },
                            Err(e) => warn!(error = ?e, "tts body error"),
                        },
                        Err(e) => warn!(error = ?e, "tts request failed"),
                    }
                    let _ = tx.send(Bytes::new());
                }
            }
            if !buf.trim().is_empty() {
                trace!(sentence = %buf, "tts final sentence");
                let url = match Mouth::tts_url(&base, &buf, &speaker_id, &lang) {
                    Ok(u) => u,
                    Err(e) => {
                        warn!(error=?e, "tts url error");
                        return;
                    }
                };
                match client.get(url).send().await {
                    Ok(resp) => match resp.bytes().await {
                        Ok(body) => match Mouth::wav_to_pcm(&body) {
                            Ok(pcm) => {
                                let _ = tx.send(pcm);
                            }
                            Err(e) => warn!(error = ?e, "wav decode failed"),
                        },
                        Err(e) => warn!(error = ?e, "tts body error"),
                    },
                    Err(e) => warn!(error = ?e, "tts request failed"),
                }
            }
            let _ = tx.send(Bytes::new());
        });
        Ok(ActionResult {
            sensations: Vec::new(),
            completed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use httpmock::prelude::*;
    use serde_json::Map;
    use serde_json::Value;

    /// Given a sentence stream, when performed, then audio is sent per sentence.
    #[tokio::test]
    async fn streams_audio_by_sentence() {
        // Arrange
        let server = MockServer::start_async().await;
        let mut wav1 = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut wav1), spec).unwrap();
            w.write_sample(1i16).unwrap();
            w.finalize().unwrap();
        }
        let mut wav2 = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut wav2), spec).unwrap();
            w.write_sample(2i16).unwrap();
            w.finalize().unwrap();
        }
        let m1 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/tts")
                    .query_param("text", "Hello world.")
                    .query_param("speaker_id", "p234")
                    .query_param("style_wav", "")
                    .query_param("language_id", "");
                then.status(200).body(wav1.clone());
            })
            .await;
        let m2 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/tts")
                    .query_param("text", "How are you?")
                    .query_param("speaker_id", "p234")
                    .query_param("style_wav", "")
                    .query_param("language_id", "");
                then.status(200).body(wav2.clone());
            })
            .await;
        let mouth = Mouth::new(server.url(""), None);
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hello world. How are you?".to_string() }).boxed();
        let mut map = Map::new();
        map.insert("speaker_id".into(), Value::String("p234".into()));
        let mut action = Action::new("speak", Value::Object(map), body);
        action.intention.assigned_motor = "speak".into();

        // Act
        mouth.perform(action).await.unwrap();
        let a = rx.recv().await.unwrap();
        let delim = rx.recv().await.unwrap();
        let b = rx.recv().await.unwrap();
        let end = rx.recv().await.unwrap();

        // Assert
        assert!(!a.is_empty());
        assert!(delim.is_empty());
        assert!(!b.is_empty());
        assert!(end.is_empty());
        m1.assert();
        m2.assert();
    }

    /// When language_id is set, it is passed to the TTS service.
    #[tokio::test]
    async fn includes_language_param() {
        // Arrange
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/tts")
                    .query_param("language_id", "en");
                then.status(200);
            })
            .await;

        let mouth = Mouth::new(server.url(""), Some("en".into()));
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let mut map = Map::new();
        map.insert("speaker_id".into(), Value::String("p234".into()));
        let mut action = Action::new("speak", Value::Object(map), body);
        action.intention.assigned_motor = "speak".into();

        // Act
        mouth.perform(action).await.unwrap();
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();

        // Assert
        mock.assert();
    }

    /// When a WAV response is received, then the header is stripped before broadcasting.
    #[tokio::test]
    async fn strips_wav_header() {
        // Arrange
        let mut wav_bytes = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer =
                hound::WavWriter::new(std::io::Cursor::new(&mut wav_bytes), spec).unwrap();
            writer.write_sample(1i16).unwrap();
            writer.finalize().unwrap();
        }
        let server = MockServer::start_async().await;
        let _mock = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(200).body(wav_bytes.clone());
            })
            .await;
        let mouth = Mouth::new(server.url(""), None);
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let mut action = Action::new("speak", Map::new().into(), body);
        action.intention.assigned_motor = "speak".into();

        // Act
        mouth.perform(action).await.unwrap();
        let mut pcm = rx.recv().await.unwrap();
        while pcm.iter().all(|b| *b == 0) {
            pcm = rx.recv().await.unwrap();
        }

        // Assert
        assert_eq!(pcm.len(), 2);
        assert_eq!(pcm.as_ref(), &[1, 0]);
    }
}
