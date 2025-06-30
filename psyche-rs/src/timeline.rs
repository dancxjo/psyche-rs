use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::{Sensation, text_util::to_plain_text};

/// Builds a textual timeline from the provided sensation window.
///
/// Sensations are sorted by timestamp, deduplicated by kind and
/// plain-text description, then formatted as lines of the form:
///
/// ```
/// 2024-02-03 12:34:56 utterance.text "hello"
/// ```
///
/// # Examples
///
/// ```
/// use std::sync::{Arc, Mutex};
/// use chrono::Local;
/// use psyche_rs::{Sensation, build_timeline};
///
/// let s = Sensation { kind: "test".into(), when: Local::now(), what: "hi".into(), source: None };
/// let tl = build_timeline(&Arc::new(Mutex::new(vec![s])));
/// assert!(tl.contains("\"hi\""));
/// ```
pub fn build_timeline<T>(window: &Arc<Mutex<Vec<Sensation<T>>>>) -> String
where
    T: Serialize + Clone,
{
    let mut sensations = window.lock().unwrap().clone();
    sensations.sort_by_key(|s| s.when);
    sensations.dedup_by(|a, b| {
        if a.kind != b.kind {
            return false;
        }
        let a_text = to_plain_text(&a.what).trim_matches('"').to_string();
        let b_text = to_plain_text(&b.what).trim_matches('"').to_string();
        a_text == b_text
    });
    sensations
        .iter()
        .map(|s| {
            let desc = to_plain_text(&s.what);
            let desc = desc.trim_matches('"');
            format!(
                "{} {} \"{}\"",
                s.when.format("%Y-%m-%d %H:%M:%S"),
                s.kind,
                desc
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    #[test]
    fn builds_sorted_deduped_timeline() {
        let s1: Sensation<String> = Sensation {
            kind: "a".into(),
            when: Local::now(),
            what: "hi".into(),
            source: None,
        };
        let s2: Sensation<String> = Sensation {
            kind: "a".into(),
            when: Local::now() + chrono::Duration::seconds(1),
            what: "hi".into(),
            source: None,
        };
        let win = Arc::new(Mutex::new(vec![s2.clone(), s1.clone()]));
        let tl = build_timeline(&win);
        let lines: Vec<_> = tl.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"hi\""));
    }
}
