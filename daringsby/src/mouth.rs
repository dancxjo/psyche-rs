use bytes::Bytes;
use futures::StreamExt;
use hound::WavReader;
use segtok::segmenter::{SegmentConfig, split_single};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{debug, error, trace, warn};

use crate::speech_segment::SpeechSegment;
#[cfg(test)]
use psyche_rs::Action;
use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError};

/// Motor that streams text-to-speech audio via HTTP.
///
/// The motor responds to the `speak` action name which streams audio via a
/// TTS service.
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
    /// HTTP client used for all TTS requests.
    client: reqwest::Client,
    base_url: String,
    language_id: Option<String>,
    tx: Sender<Bytes>,
    text_tx: Sender<String>,
    segment_tx: Sender<SpeechSegment>,
    queue: Arc<TokioMutex<()>>,
}

impl Default for Mouth {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(64);
        let (text_tx, _) = broadcast::channel(64);
        let (segment_tx, _) = broadcast::channel(64);
        let queue = Arc::new(TokioMutex::new(()));
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: "http://localhost:5002".into(),
            language_id: None,
            tx,
            text_tx,
            segment_tx,
            queue,
        }
    }
}

impl Mouth {
    /// Creates a mouth with the given HTTP client, base URL and optional language.
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        language_id: Option<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(64);
        let (text_tx, _) = broadcast::channel(64);
        let (segment_tx, _) = broadcast::channel(64);
        let queue = Arc::new(TokioMutex::new(()));
        Self {
            client,
            base_url: base_url.into(),
            language_id,
            tx,
            text_tx,
            segment_tx,
            queue,
        }
    }

    /// Subscribes to the audio stream.
    pub fn subscribe(&self) -> Receiver<Bytes> {
        self.tx.subscribe()
    }

    /// Subscribes to the text stream for spoken sentences.
    pub fn subscribe_text(&self) -> Receiver<String> {
        self.text_tx.subscribe()
    }

    /// Subscribe to combined speech segments of text and audio.
    pub fn subscribe_segments(&self) -> Receiver<SpeechSegment> {
        self.segment_tx.subscribe()
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
        "Speak text out loud to your interlocutor (or no one at all).\n\
Params: `speaker_id` (required) and required `language_id`.\n\
Example:\n\
<speak speaker_id=\"p234\" language_id=\"en\">Hello, world.</speak>\n\
Explanation:\n\
The Will sends the text to the TTS service with the given voice and language."
    }

    fn name(&self) -> &'static str {
        "speak"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "speak" {
            return Err(MotorError::Unrecognized);
        }
        let mut action = intention.action;
        let speaker_id = action
            .params
            .get("speaker_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some("p234".into()))
            .unwrap_or_default();
        let lang = action
            .params
            .get("language_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.language_id.clone())
            .unwrap_or_default();
        let client = self.client.clone();
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        let text_tx = self.text_tx.clone();
        let segment_tx = self.segment_tx.clone();
        let queue = self.queue.clone();
        let lang = lang;
        {
            let _guard = queue.lock().await;
            let mut buf = String::new();
            let mut chunks = action.logged_chunks();
            while let Some(chunk) = chunks.next().await {
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
                    let desc = format!("tts text {} bytes", sent.len());
                    if let Err(e) = text_tx.send(sent.clone()) {
                        warn!(target: "mouth", error=?e, what=%desc, "broadcast send failed");
                    }
                    let url = match Mouth::tts_url(&base, &sent, &speaker_id, &lang) {
                        Ok(u) => u,
                        Err(e) => {
                            error!(error=?e, "tts url error");
                            continue;
                        }
                    };
                    match client.get(url).send().await {
                        Ok(resp) => {
                            let mut body = Vec::new();
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(b) => body.extend_from_slice(&b),
                                    Err(e) => {
                                        error!(error=?e, "tts stream error");
                                        break;
                                    }
                                }
                            }
                            match Mouth::wav_to_pcm(&body) {
                                Ok(pcm) => {
                                    let seg = SpeechSegment::new(&sent, &pcm);
                                    let seg_desc = format!("speech segment {} bytes", pcm.len());
                                    if let Err(e) = segment_tx.send(seg) {
                                        warn!(target: "mouth", error=?e, what=%seg_desc, "broadcast send failed");
                                    }
                                    let audio_desc = format!("pcm {} bytes", pcm.len());
                                    if let Err(e) = tx.send(pcm) {
                                        warn!(target: "mouth", error=?e, what=%audio_desc, "broadcast send failed");
                                    }
                                }
                                Err(e) => error!(error=?e, "wav decode failed"),
                            }
                        }
                        Err(e) => error!(error=?e, "tts request failed"),
                    }
                    if let Err(e) = tx.send(Bytes::new()) {
                        warn!(target: "mouth", error=?e, what="silence frame", "broadcast send failed");
                    }
                }
            }
            if !buf.trim().is_empty() {
                trace!(sentence = %buf, "tts final sentence");
                let desc = format!("tts text {} bytes", buf.len());
                if let Err(e) = text_tx.send(buf.clone()) {
                    warn!(target: "mouth", error=?e, what=%desc, "broadcast send failed");
                }
                if let Ok(url) = Mouth::tts_url(&base, &buf, &speaker_id, &lang) {
                    match client.get(url).send().await {
                        Ok(resp) => {
                            let mut body = Vec::new();
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(b) => body.extend_from_slice(&b),
                                    Err(e) => {
                                        error!(error=?e, "tts stream error");
                                        break;
                                    }
                                }
                            }
                            match Mouth::wav_to_pcm(&body) {
                                Ok(pcm) => {
                                    let seg = SpeechSegment::new(&buf, &pcm);
                                    let seg_desc = format!("speech segment {} bytes", pcm.len());
                                    if let Err(e) = segment_tx.send(seg) {
                                        warn!(target: "mouth", error=?e, what=%seg_desc, "broadcast send failed");
                                    }
                                    let audio_desc = format!("pcm {} bytes", pcm.len());
                                    if let Err(e) = tx.send(pcm) {
                                        warn!(target: "mouth", error=?e, what=%audio_desc, "broadcast send failed");
                                    }
                                }
                                Err(e) => error!(error=?e, "wav decode failed"),
                            }
                        }
                        Err(e) => error!(error=?e, "tts request failed"),
                    }
                } else {
                    // invalid URL, skip final sentence
                }
            }
            if let Err(e) = tx.send(Bytes::new()) {
                warn!(target: "mouth", error=?e, what="silence frame", "broadcast send failed");
            }
        }
        let completion = Completion::of_action(action);
        debug!(
            completion_name = %completion.name,
            completion_params = ?completion.params,
            completion_result = ?completion.result,
            ?completion,
            "action completed"
        );
        Ok(ActionResult {
            sensations: Vec::new(),
            completed: true,
            completion: Some(completion),
            interruption: None,
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
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let mouth = Mouth::new(client, server.url(""), None);
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hello world. How are you?".to_string() }).boxed();
        let mut map = Map::new();
        map.insert("speaker_id".into(), Value::String("p234".into()));
        let action = Action::new("speak", Value::Object(map), body);
        let intention = Intention::to(action).assign("speak");

        // Act
        mouth.perform(intention).await.unwrap();
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

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let mouth = Mouth::new(client, server.url(""), Some("en".into()));
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let mut map = Map::new();
        map.insert("speaker_id".into(), Value::String("p234".into()));
        let action = Action::new("speak", Value::Object(map), body);
        let intention = Intention::to(action).assign("speak");

        // Act
        mouth.perform(intention).await.unwrap();
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();

        // Assert
        assert!(mock.hits_async().await > 0);
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
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let mouth = Mouth::new(client, server.url(""), None);
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let action = Action::new("speak", Map::new().into(), body);
        let intention = Intention::to(action).assign("speak");

        // Act
        mouth.perform(intention).await.unwrap();
        let mut pcm = rx.recv().await.unwrap();
        while pcm.iter().all(|b| *b == 0) {
            pcm = rx.recv().await.unwrap();
        }

        // Assert
        assert_eq!(pcm.len(), 2);
        assert_eq!(pcm.as_ref(), &[1, 0]);
    }

    /// Segments include text with base64 audio.
    #[tokio::test]
    async fn emits_segments() {
        let server = MockServer::start_async().await;
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
        let _mock = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(200).body(wav_bytes.clone());
            })
            .await;

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let mouth = Mouth::new(client, server.url(""), None);
        let mut seg_rx = mouth.subscribe_segments();
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let action = Action::new("speak", Map::new().into(), body);
        let intention = Intention::to(action).assign("speak");
        mouth.perform(intention).await.unwrap();
        let seg = seg_rx.recv().await.unwrap();
        assert_eq!(seg.text, "Hi.");
        let decoded = seg.decode_audio().unwrap();
        assert!(!decoded.is_empty());
    }

    #[tokio::test]
    async fn perform_returns_completion() {
        let server = MockServer::start_async().await;
        let _mock = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(200).body(Vec::new());
            })
            .await;
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap();
        let mouth = Mouth::new(client, server.url(""), None);
        let body = stream::once(async { "Hi.".to_string() }).boxed();
        let action = Action::new("speak", Map::new().into(), body);
        let intention = Intention::to(action).assign("speak");
        let result = mouth.perform(intention).await.unwrap();
        assert!(result.completed);
        let completion = result.completion.unwrap();
        assert_eq!(completion.name, "speak");
        assert!(completion.params.as_object().unwrap().is_empty());
    }
}
