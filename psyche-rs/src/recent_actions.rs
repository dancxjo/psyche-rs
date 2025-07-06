use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Thread-safe log of recent action summaries.
#[derive(Debug)]
pub struct RecentActionsLog {
    entries: Mutex<VecDeque<(DateTime<Utc>, String)>>,
    cap: usize,
}

impl RecentActionsLog {
    /// Create a new log keeping at most `cap` summaries.
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
        }
    }

    /// Append an action summary.
    pub fn push(&self, summary: impl Into<String>) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() == self.cap {
            entries.pop_front();
        }
        entries.push_back((Utc::now(), summary.into()));
    }

    /// Take and clear all stored summaries.
    pub fn take_all(&self) -> Vec<String> {
        let mut entries = self.entries.lock().unwrap();
        entries.drain(..).map(|(_, s)| s).collect()
    }
}

impl Default for RecentActionsLog {
    fn default() -> Self {
        Self::new(8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_take() {
        let log = RecentActionsLog::new(2);
        log.push("a");
        log.push("b");
        log.push("c");
        let got = log.take_all();
        assert_eq!(got, vec!["b", "c"]);
        assert!(log.take_all().is_empty());
    }
}
