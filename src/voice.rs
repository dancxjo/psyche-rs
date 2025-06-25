/// Pete's voice system used to craft text responses.
///
/// The voice maintains optional context about Pete's current mood in order to
/// flavour its next utterance.
///
/// # Examples
///
/// ```
/// use psyche_rs::voice::Voice;
///
/// let mut voice = Voice::default();
/// assert_eq!(voice.prompt(), "😐 You said: ...");
/// voice.update_mood("😊".to_string());
/// assert!(voice.prompt().starts_with("😊"));
/// ```
#[derive(Default)]
pub struct Voice {
    /// Latest emotional tone to express with the next prompt.
    pub current_mood: Option<String>,
}

impl Voice {
    /// Update the currently expressed mood.
    pub fn update_mood(&mut self, mood: String) {
        self.current_mood = Some(mood);
    }

    /// Compose a prompt incorporating the current mood.
    pub fn prompt(&self) -> String {
        let mood = self.current_mood.as_deref().unwrap_or("😐");
        format!("{} You said: ...", mood)
    }
}
