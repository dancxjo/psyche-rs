use psyche_rs::{Emotion, countenance::Countenance, voice::Voice};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use uuid::Uuid;

struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
}

impl Countenance for Recorder {
    fn reflect(&self, mood: &str) {
        self.0.lock().unwrap().push(mood.to_string());
    }
}

#[test]
fn emotion_updates_voice_and_countenance() {
    let mut voice = Voice::default();
    let rec = Recorder::new();

    let emotion = Emotion {
        uuid: Uuid::new_v4(),
        subject: Uuid::new_v4(),
        mood: "joyful".into(),
        reason: "tests passed".into(),
        timestamp: SystemTime::now(),
    };

    // mimic runtime reaction
    voice.update_mood(emotion.mood.clone());
    rec.reflect(&emotion.mood);

    assert!(voice.prompt().starts_with("joyful"));
    assert_eq!(rec.0.lock().unwrap()[0], "joyful");
}
