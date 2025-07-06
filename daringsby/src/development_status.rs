use async_stream::stream;
use once_cell::sync::Lazy;
use segtok::segmenter::{SegmentConfig, split_single};
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

const DEVELOPMENT_TEXT: &str = include_str!("development_status.txt");

static DEVELOPMENT_SENTENCES: Lazy<Vec<String>> = Lazy::new(|| {
    split_single(DEVELOPMENT_TEXT, SegmentConfig::default())
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
});

/// Sensor that reminds Pete he is under active development.
///
/// Emits the contents of `development_status.txt` as a single sensation
/// roughly every minute.
///
/// # Examples
/// ```
/// use futures::StreamExt;
/// use daringsby::development_status::DevelopmentStatus;
/// use psyche_rs::Sensor;
/// use tokio::runtime::Runtime;
///
/// let rt = Runtime::new().unwrap();
/// rt.block_on(async {
///     unsafe { std::env::set_var("FAST_TEST", "1") };
///     let mut sensor = DevelopmentStatus;
///     let mut stream = sensor.stream();
///     if let Some(batch) = stream.next().await {
///         assert!(batch[0].what.contains("still under development"));
///     }
/// });
/// ```
pub struct DevelopmentStatus;

impl Sensor<String> for DevelopmentStatus {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let sentences = DEVELOPMENT_SENTENCES.clone();
        let mut index = 0usize;
        let len = sentences.len();
        let stream = stream! {
            loop {
                if std::env::var("FAST_TEST").is_err() {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
                debug!("development status sensed");
                let text = sentences[index].clone();
                index = (index + 1) % len;
                let s = Sensation {
                    kind: "development_status".into(),
                    when: chrono::Local::now(),
                    what: text,
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
    use futures::StreamExt;

    #[tokio::test]
    async fn emits_message() {
        unsafe { std::env::set_var("FAST_TEST", "1") };
        let mut sensor = DevelopmentStatus;
        let mut stream = sensor.stream();
        let batch1 = stream.next().await.expect("first batch");
        assert!(batch1[0].what.contains("still under development"));

        let batch2 = stream.next().await.expect("second batch");
        assert_ne!(batch2[0].what, batch1[0].what);
    }
}
