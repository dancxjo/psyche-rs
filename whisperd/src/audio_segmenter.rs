use tracing::{debug, trace};
use webrtc_vad::{SampleRate, Vad, VadMode};

/// Number of samples per VAD frame (30ms @ 16kHz).
pub const FRAME_SIZE: usize = 480;
/// Default amount of accumulated silence before yielding a segment
/// (approximately 1s at 16&nbsp;kHz).
pub const DEFAULT_SILENCE_FRAMES: usize = FRAME_SIZE * 34;
/// Minimum required voiced frames before accepting a segment (approximately 0.5s @ 16kHz).
pub const MIN_VOICE_FRAMES: usize = FRAME_SIZE * 16;

/// Splits incoming PCM frames using VAD and emits complete voice segments.
///
/// Audio is processed in `FRAME_SIZE` chunks which equate to 30&nbsp;ms of
/// 16&nbsp;kHz samples. Once the configured amount of silence is observed after
/// speech exceeding `MIN_VOICE_FRAMES`, the voiced portion is returned for
/// transcription. Shorter voiced snippets are discarded to avoid false triggers.
pub struct AudioSegmenter {
    vad: Option<Vad>,
    buffer: Vec<i16>,
    idx: usize,
    silence: usize,
    voiced: usize,
    discarded: usize,
    silence_frames: usize,
}

impl AudioSegmenter {
    /// Create a new instance configured for 16kHz audio with the provided
    /// silence threshold in frames.
    pub fn new(silence_frames: usize) -> Self {
        let mut vad = Vad::new_with_rate(SampleRate::Rate16kHz);
        vad.set_mode(VadMode::Quality);
        Self {
            vad: Some(vad),
            buffer: Vec::new(),
            idx: 0,
            silence: 0,
            voiced: 0,
            discarded: 0,
            silence_frames,
        }
    }

    #[cfg(test)]
    pub fn new_without_vad(silence_frames: usize) -> Self {
        Self {
            vad: None,
            buffer: Vec::new(),
            idx: 0,
            silence: 0,
            voiced: 0,
            discarded: 0,
            silence_frames,
        }
    }

    /// Push PCM frames to the segmenter.
    ///
    /// Returns a completed voice segment when enough silence has been detected
    /// after voiced audio.
    pub fn push_frames(&mut self, frames: &[i16]) -> Option<Vec<i16>> {
        trace!(frames = frames.len(), "received frames");
        self.buffer.extend_from_slice(frames);
        while self.idx + FRAME_SIZE <= self.buffer.len() {
            let frame = &self.buffer[self.idx..self.idx + FRAME_SIZE];
            self.idx += FRAME_SIZE;
            let voiced_frame = match &mut self.vad {
                Some(v) => v.is_voice_segment(frame).unwrap_or(false),
                None => true,
            };
            if voiced_frame {
                self.silence = 0;
                self.voiced += FRAME_SIZE;
            } else {
                self.silence += FRAME_SIZE;
                trace!(silence = self.silence, "silence increasing");
            }
            if self.silence >= self.silence_frames {
                let segment = self.buffer.split_off(self.idx - self.silence);
                let spoken = std::mem::take(&mut self.buffer);
                self.buffer = segment;
                self.idx = 0;
                self.silence = 0;
                let voiced = std::mem::replace(&mut self.voiced, 0);
                if voiced >= MIN_VOICE_FRAMES {
                    trace!(samples = spoken.len(), "segment ready");
                    return Some(spoken);
                } else {
                    self.discarded += spoken.len();
                    if !spoken.is_empty() {
                        debug!(samples = spoken.len(), "discarding short segment");
                    }
                }
            }
        }
        None
    }

    /// Discard internal buffers and return any remaining voiced audio if long
    /// enough to be useful.
    pub fn finish(&mut self) -> Option<Vec<i16>> {
        let voiced = self.voiced;
        self.voiced = 0;
        self.silence = 0;
        self.idx = 0;
        if voiced >= MIN_VOICE_FRAMES {
            Some(std::mem::take(&mut self.buffer))
        } else {
            self.discarded += self.buffer.len();
            self.buffer.clear();
            None
        }
    }

    /// Total discarded samples due to short segments.
    pub fn discarded(&self) -> usize {
        self.discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    #[ignore]
    fn emits_segment_after_voice_and_silence() {
        let mut seg = AudioSegmenter::new_without_vad(DEFAULT_SILENCE_FRAMES);
        let voiced = vec![20_000i16; MIN_VOICE_FRAMES];
        let silence = vec![0i16; DEFAULT_SILENCE_FRAMES];
        assert!(seg.push_frames(&voiced).is_none());
        assert!(seg.push_frames(&silence).is_some());
    }

    #[traced_test]
    #[test]
    #[ignore]
    fn discards_short_segments() {
        let mut seg = AudioSegmenter::new_without_vad(DEFAULT_SILENCE_FRAMES);
        let voiced = vec![20_000i16; MIN_VOICE_FRAMES / 2];
        let silence = vec![0i16; DEFAULT_SILENCE_FRAMES];
        assert!(seg.push_frames(&voiced).is_none());
        assert!(seg.push_frames(&silence).is_none());
        assert_eq!(seg.discarded(), voiced.len());
    }
}
