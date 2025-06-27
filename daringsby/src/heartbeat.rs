use async_stream::stream;
use rand::Rng;
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

/// Formats a heartbeat message from the provided time.
///
/// # Examples
/// ```
/// use chrono::{Local, TimeZone};
/// use daringsby::heartbeat::heartbeat_message;
/// let dt = Local.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
/// assert_eq!(
///     heartbeat_message(dt),
///     "It's 2024-01-01 12:00:00 +00:00, and I felt my heart beat, so I know I'm alive."
/// );
/// assert_eq!(heartbeat_message(dt), expected);
/// ```
pub fn heartbeat_message(now: chrono::DateTime<chrono::Local>) -> String {
    format!(
        "It's {}, and I felt my heart beat, so I know I'm alive.",
        now.format("%Y-%m-%d %H:%M:%S %Z").to_string()
    )
}

/// A sensor that emits a heartbeat sensation roughly every minute.
pub struct Heartbeat;

impl Sensor<String> for Heartbeat {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let stream = stream! {
            loop {
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..15)
                };
                tokio::time::sleep(std::time::Duration::from_secs(3 + jitter)).await;
                let now = chrono::Local::now();
                let msg = heartbeat_message(now);
                debug!(?msg, "heartbeat sensed");
                let s = Sensation {
                    kind: "heartbeat".into(),
                    when: chrono::Utc::now(),
                    what: msg,
                    source: None,
                };
                yield vec![s];
            }
        };
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn formats_message() {
        let dt = chrono::Local.with_ymd_and_hms(2024, 1, 1, 8, 0, 0).unwrap();
        let msg = heartbeat_message(dt);
        let expected = format!(
            "It's {}, and I felt my heart beat, so I know I'm alive.",
            dt.format("%Y-%m-%d %H:%M:%S %Z")
        );
        assert_eq!(msg, expected);
    }
}
