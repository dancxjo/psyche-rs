use serde::{Deserialize, Serialize};

use crate::sensation::Sensation;

/// A high-level summary of one or more sensations.
///
/// `Impression` bundles related sensations in [`what`] and expresses
/// them in natural language via [`how`]. `how` should be a single,
/// complete sentence.
///
/// # Examples
///
/// ```
/// use chrono::Local;
/// use psyche_rs::{Impression, Sensation};
///
/// let what = vec![Sensation::<String> {
///     kind: "utterance.text".into(),
///     when: Local::now(),
///     what: "salutations".into(),
///     source: None,
/// }];
/// let impression = Impression::new(what.clone(), "He said salutations.")
///     .unwrap();
/// assert_eq!(impression.what[0].kind, "utterance.text");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impression<T = serde_json::Value> {
    /// Sensations that led to this impression.
    pub what: Vec<Sensation<T>>,
    /// Natural language summarizing the sensations. Must be exactly one
    /// sentence so it can be easily vectorized and reasoned about.
    pub how: String,
}

impl<T: Default> Default for Impression<T> {
    fn default() -> Self {
        Self {
            what: Vec::new(),
            how: String::new(),
        }
    }
}

impl<T> Impression<T> {
    /// Create a new impression ensuring `how` contains exactly one
    /// sentence.
    ///
    /// # Errors
    ///
    /// Returns an error if `how` does not contain exactly one sentence.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::Local;
    /// use psyche_rs::{Impression, Sensation};
    ///
    /// let what = vec![Sensation::<String> {
    ///     kind: "utterance.text".into(),
    ///     when: Local::now(),
    ///     what: "hello".into(),
    ///     source: None,
    /// }];
    /// let imp = Impression::new(what, "He said hello.").unwrap();
    /// assert_eq!(imp.how, "He said hello.");
    /// ```
    pub fn new(what: Vec<Sensation<T>>, how: impl Into<String>) -> anyhow::Result<Self> {
        let how = how.into();
        let sentences: Vec<String> =
            segtok::segmenter::split_single(&how, segtok::segmenter::SegmentConfig::default())
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        if sentences.len() != 1 {
            anyhow::bail!("`how` must contain exactly one sentence");
        }
        Ok(Self {
            what,
            how: sentences[0].clone(),
        })
    }

    /// Iterator over kinds of contained sensations.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.what.iter().map(|s| s.kind.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn example_usage() {
        let what = vec![Sensation::<String> {
            kind: "utterance.text".into(),
            when: Local.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            what: "salutations".into(),
            source: None,
        }];
        let impression = Impression::new(what.clone(), "He said salutations.").unwrap();
        assert_eq!(impression.what[0].kind, "utterance.text");
        assert_eq!(
            impression.kinds().collect::<Vec<_>>(),
            vec!["utterance.text"]
        );
    }

    #[test]
    fn default_is_empty() {
        let imp = Impression::<String>::default();
        assert!(imp.how.is_empty());
        assert!(imp.what.is_empty());
    }
}
