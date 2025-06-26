use psyche_rs::{countenance::Countenance, voice::Voice};
use std::sync::{Arc, Mutex};

struct Recorder {
    log: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Self { log: log.clone() }, log)
    }
}

impl Countenance for Recorder {
    fn reflect(&self, mood: &str) {
        self.log.lock().unwrap().push(mood.to_string());
    }
}

#[test]
fn voice_incorporates_mood() {
    let mut voice = Voice::default();
    assert_eq!(voice.prompt(), "😐 You said: ...");
    voice.update_mood("happy".into());
    assert!(voice.prompt().starts_with("happy"));
}

#[test]
fn countenance_records_reflection() {
    let (rec, log) = Recorder::new();
    rec.reflect("excited");
    assert_eq!(log.lock().unwrap()[0], "excited");
}
