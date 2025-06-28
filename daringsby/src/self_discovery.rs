use async_stream::stream;
use once_cell::sync::Lazy;
use rand::Rng;
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

const SELF_DISCOVERY_TEXT: &str = include_str!("self_discovery.txt");

static SENTENCES: Lazy<Vec<&'static str>> = Lazy::new(|| {
    SELF_DISCOVERY_TEXT
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
});

/// Sensor that reveals Pete's self narrative one sentence at a time.
///
/// # Examples
/// ```
/// use futures::StreamExt;
/// use daringsby::self_discovery::SelfDiscovery;
/// use psyche_rs::Sensor;
/// use tokio::runtime::Runtime;
///
/// let rt = Runtime::new().unwrap();
/// rt.block_on(async {
///     unsafe { std::env::set_var("FAST_TEST", "1") };
///     let mut sensor = SelfDiscovery::default();
///     let mut stream = sensor.stream();
///     if let Some(batch) = stream.next().await {
///         assert_eq!(batch[0].kind, "self_discovery");
///     }
/// });
/// ```
pub struct SelfDiscovery;

fn interval_secs() -> u64 {
    if std::env::var("FAST_TEST").is_ok() {
        0
    } else {
        60
    }
}

impl Default for SelfDiscovery {
    fn default() -> Self {
        Self
    }
}

impl Sensor<String> for SelfDiscovery {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let sentences = SENTENCES.clone();
        let stream = stream! {
            let mut index = 0usize;
            loop {
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..2)
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs() + jitter)).await;
                let msg = sentences[index].to_string();
                index = (index + 1) % sentences.len();
                debug!(?msg, "self discovery sensed");
                let s = Sensation {
                    kind: "self_discovery".into(),
                    when: chrono::Utc::now(),
                    what: format!("I hear a voice inside my mind say: \"{}\"", msg),
                    source: Some("voice_inside_my_mind".into()),
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
    use futures::StreamExt;

    #[tokio::test]
    async fn emits_first_sentence() {
        unsafe { std::env::set_var("FAST_TEST", "1") };
        let mut sensor = SelfDiscovery;
        let mut stream = sensor.stream();
        if let Some(batch) = stream.next().await {
            let expected = format!(
                "I hear a voice inside my mind say: \"{}\"",
                SENTENCES[0]
            );
            assert_eq!(batch[0].what, expected);
        } else {
            panic!("no sentence emitted");
        }
    }
}
