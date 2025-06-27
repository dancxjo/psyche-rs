use bytes::Bytes;
use futures::StreamExt;
use segtok::segmenter::{SegmentConfig, split_single};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{trace, warn};
use urlencoding::encode;

use psyche_rs::{Action, Motor, MotorError};

/// Motor that streams text-to-speech audio via HTTP.
///
/// Sentences from the input body are sent to a TTS service and the
/// resulting audio bytes are broadcast to subscribers.
///
/// # Example
/// ```
/// use daringsby::Mouth;
/// let mouth = Mouth::default();
/// let _ = mouth.subscribe();
/// ```
pub struct Mouth {
    client: reqwest::Client,
    base_url: String,
    language_id: Option<String>,
    tx: Sender<Bytes>,
}

impl Default for Mouth {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(8);
        Self {
            client: reqwest::Client::new(),
            base_url: "http://10.0.0.180:5002".into(),
            language_id: None,
            tx,
        }
    }
}

impl Mouth {
    /// Creates a mouth with the given base URL and optional language.
    pub fn new(base_url: impl Into<String>, language_id: Option<String>) -> Self {
        let (tx, _) = broadcast::channel(8);
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            language_id,
            tx,
        }
    }

    /// Subscribes to the audio stream.
    pub fn subscribe(&self) -> Receiver<Bytes> {
        self.tx.subscribe()
    }

    fn tts_url(base: &str, text: &str, speaker_id: &str, language: &str) -> String {
        format!(
            "{}/api/tts?text={}&speaker_id={}&style_wav=&language_id={}",
            base,
            encode(text),
            speaker_id,
            language
        )
    }
}

impl Motor for Mouth {
    fn description(&self) -> &'static str {
        "Streams TTS audio from text via HTTP"
    }

    fn perform(&self, mut action: Action) -> Result<(), MotorError> {
        if action.name != "speak" {
            return Err(MotorError::Unrecognized);
        }
        let speaker_id = action
            .params
            .get("speaker_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MotorError::Failed("speaker_id required".into()))?
            .to_string();
        let lang = action
            .params
            .get("language_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.language_id.clone())
            .unwrap_or_default();
        let client = self.client.clone();
        let lang = lang.clone();
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
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
                    let url = Mouth::tts_url(&base, &sent, &speaker_id, &lang);
                    match client.get(url).send().await {
                        Ok(resp) => {
                            let mut stream = resp.bytes_stream();
                            while let Some(Ok(bytes)) = stream.next().await {
                                let _ = tx.send(bytes);
                            }
                        }
                        Err(e) => warn!(error = ?e, "tts request failed"),
                    }
                    let _ = tx.send(Bytes::new());
                }
            }
            if !buf.trim().is_empty() {
                trace!(sentence = %buf, "tts final sentence");
                let url = Mouth::tts_url(&base, &buf, &speaker_id, &lang);
                match client.get(url).send().await {
                    Ok(resp) => {
                        let mut stream = resp.bytes_stream();
                        while let Some(Ok(bytes)) = stream.next().await {
                            let _ = tx.send(bytes);
                        }
                    }
                    Err(e) => warn!(error = ?e, "tts request failed"),
                }
            }
            let _ = tx.send(Bytes::new());
        });
        Ok(())
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
        let m1 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/tts")
                    .query_param("text", "Hello world.")
                    .query_param("speaker_id", "p1")
                    .query_param("style_wav", "")
                    .query_param("language_id", "");
                then.status(200).body("A");
            })
            .await;
        let m2 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/tts")
                    .query_param("text", "How are you?")
                    .query_param("speaker_id", "p1")
                    .query_param("style_wav", "")
                    .query_param("language_id", "");
                then.status(200).body("B");
            })
            .await;
        let mouth = Mouth::new(server.url(""), None);
        let mut rx = mouth.subscribe();
        let body = stream::once(async { "Hello world. How are you?".to_string() }).boxed();
        let mut map = Map::new();
        map.insert("speaker_id".into(), Value::String("p1".into()));
        let action = Action::new("speak", Value::Object(map), body);

        // Act
        mouth.perform(action).unwrap();
        let a = rx.recv().await.unwrap();
        let delim = rx.recv().await.unwrap();
        let b = rx.recv().await.unwrap();
        let end = rx.recv().await.unwrap();

        // Assert
        assert_eq!(a.as_ref(), b"A");
        assert!(delim.is_empty());
        assert_eq!(b.as_ref(), b"B");
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
        map.insert("speaker_id".into(), Value::String("p1".into()));
        let action = Action::new("speak", Value::Object(map), body);

        // Act
        mouth.perform(action).unwrap();
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();

        // Assert
        mock.assert();
    }
}
