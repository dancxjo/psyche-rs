use async_stream::stream;
use include_dir::{Dir, include_dir};
use once_cell::sync::Lazy;
use rand::Rng;
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

/// Directory containing the crate's source code packaged into the binary.
static SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src");

/// The source code chunks that will be revealed sequentially.
static CHUNKS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    SRC_DIR
        .files()
        .filter_map(|f| {
            if f.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                f.contents_utf8()
            } else {
                None
            }
        })
        .collect()
});

/// Sensor that emits large chunks of the crate's source code.
///
/// Each batch contains a single sensation with the text
/// `"I know that this is from my own source code: {chunk}"` where `{chunk}`
/// is the contents of a Rust source file. The sensor cycles through all
/// `.rs` files in the crate, sleeping for one or two seconds (with jitter)
/// between emissions.
///
/// # Examples
/// ```
/// use futures::StreamExt;
/// use daringsby::source_discovery::SourceDiscovery;
/// use psyche_rs::Sensor;
/// use tokio::runtime::Runtime;
///
/// let rt = Runtime::new().unwrap();
/// rt.block_on(async {
///     let mut sensor = SourceDiscovery::default();
///     let mut stream = sensor.stream();
///     if let Some(batch) = stream.next().await {
///         assert!(batch[0].what.starts_with("I know that this is from my own source code:"));
///     }
/// });
/// ```
#[derive(Default)]
pub struct SourceDiscovery;

impl Sensor<String> for SourceDiscovery {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let chunks = CHUNKS.clone();
        let stream = stream! {
            let mut index = 0usize;
            loop {
                let jitter = { let mut rng = rand::thread_rng(); rng.gen_range(0..2) };
                tokio::time::sleep(std::time::Duration::from_secs(1 + jitter)).await;
                let chunk = chunks[index];
                index = (index + 1) % chunks.len();
                debug!(?index, "self source sensed");
                let msg = format!("I know that this is from my own source code:\n{}", chunk);
                let s = Sensation {
                    kind: "self_source".into(),
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
    use futures::StreamExt;

    #[tokio::test]
    async fn emits_first_chunk() {
        let mut sensor = SourceDiscovery::default();
        let mut stream = sensor.stream();
        if let Some(batch) = stream.next().await {
            let expected = format!(
                "I know that this is from my own source code:\n{}",
                CHUNKS[0]
            );
            assert_eq!(batch[0].what, expected);
        } else {
            panic!("no chunk emitted");
        }
    }
}
