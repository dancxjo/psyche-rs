use async_stream::stream;
use include_dir::{Dir, include_dir};
use once_cell::sync::Lazy;
use rand::Rng;
use tracing::debug;

use psyche_rs::{Sensation, Sensor};

/// Directory containing the crate's source code packaged into the binary.
static DARINGSBY_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src");
/// Directory containing the psyche-rs crate's source code packaged into the binary.
static PSYCHE_SRC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../psyche-rs/src");

/// The source code chunks that will be revealed sequentially.
/// Maximum number of lines yielded in a single chunk.
const MAX_LINES: usize = 20;

fn interval_secs() -> u64 {
    if std::env::var("FAST_TEST").is_ok() {
        0
    } else {
        60
    }
}

/// Source code chunks that will be revealed sequentially from both the
/// `daringsby` and `psyche-rs` crates. Files are split into reasonably sized
/// segments so the agent only sees a portion at a time.
static CHUNKS: Lazy<Vec<String>> = Lazy::new(|| {
    let mut pieces = Vec::new();
    for dir in [&DARINGSBY_SRC_DIR, &PSYCHE_SRC_DIR] {
        for file in dir.files() {
            if file.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Some(text) = file.contents_utf8() {
                    let lines: Vec<&str> = text.lines().collect();
                    for chunk in lines.chunks(MAX_LINES) {
                        pieces.push(chunk.join("\n"));
                    }
                }
            }
        }
    }
    pieces
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
///     unsafe { std::env::set_var("FAST_TEST", "1") };
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
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..2)
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs() + jitter)).await;
                let chunk = &chunks[index];
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
        unsafe { std::env::set_var("FAST_TEST", "1") };
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

    #[test]
    fn contains_psyche_chunk() {
        assert!(CHUNKS.iter().any(|c| c.contains("psyche-rs")));
    }
}
