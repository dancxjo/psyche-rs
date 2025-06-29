use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Combined speech segment containing the spoken text and its PCM audio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechSegment {
    /// The sentence that was synthesized.
    pub text: String,
    /// Base64 encoded PCM bytes.
    pub audio_b64: String,
}

impl SpeechSegment {
    /// Create a segment from raw PCM bytes.
    pub fn new(text: impl Into<String>, audio: &[u8]) -> Self {
        Self {
            text: text.into(),
            audio_b64: STANDARD.encode(audio),
        }
    }

    /// Decode the stored audio into raw PCM bytes.
    ///
    /// ```
    /// use daringsby::speech_segment::SpeechSegment;
    /// let seg = SpeechSegment::new("hi", &[1u8, 2]);
    /// assert_eq!(seg.decode_audio().unwrap(), bytes::Bytes::from_static(&[1, 2]));
    /// ```
    pub fn decode_audio(&self) -> Result<Bytes, base64::DecodeError> {
        STANDARD.decode(&self.audio_b64).map(Bytes::from)
    }
}
