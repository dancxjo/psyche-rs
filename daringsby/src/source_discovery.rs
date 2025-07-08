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
/// Project README packaged into the binary.
const README_TEXT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md"));

/// The source code chunks that will be revealed sequentially.
/// Maximum number of lines yielded in a single chunk.
const MAX_LINES: usize = 20;

fn interval_secs() -> u64 {
    if std::env::var("FAST_TEST").is_ok() {
        0
    } else {
        420
    }
}

/// Source code chunks revealed sequentially from the `daringsby` and `psyche-rs`
/// crates along with the project README. Files are split into reasonably sized
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
    // Split README into chunks
    for chunk in README_TEXT.lines().collect::<Vec<_>>().chunks(MAX_LINES) {
        pieces.push(chunk.join("\n"));
    }
    if pieces.is_empty() {
        panic!("CHUNKS is empty: no source code was included at compile time");
    }
    pieces
});

#[doc(hidden)]
pub fn set_fast_test_env() {
    // SAFETY: tests run single-threaded so modifying environment variables is
    // safe in this context.
    unsafe { std::env::set_var("FAST_TEST", "1") };
}

/// Sensor that emits large chunks of the crate's source code.
///
/// The entire `daringsby` and `psyche-rs` source trees are embedded in the
/// compiled binary via `include_dir!`. This means binaries produced with this
/// sensor contain all project source code, which can noticeably increase their
/// size.
///
/// Each batch contains a single sensation with the text
/// `"I know that this is from my own source code: {chunk}"` where `{chunk}`
/// is the contents of a Rust source file. The sensor cycles through all
/// `.rs` files in the crate, sleeping for roughly seven minutes between
/// emissions.
///
/// Set `SOURCE_DISCOVERY_ABORT=1` to abort the stream early or
/// `SOURCE_DISCOVERY_CYCLES=N` to stop after `N` full iterations over all
/// chunks.
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
///     daringsby::source_discovery::set_fast_test_env();
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
            let mut cycles = 0usize;
            let max_cycles = std::env::var("SOURCE_DISCOVERY_CYCLES")
                .ok()
                .and_then(|v| v.parse::<usize>().ok());
            loop {
                if std::env::var("SOURCE_DISCOVERY_ABORT").is_ok() {
                    break;
                }
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..2)
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs() + jitter)).await;
                let chunk = &chunks[index];
                index = (index + 1) % chunks.len();
                if index == 0 {
                    cycles += 1;
                    if let Some(max) = max_cycles {
                        if cycles >= max {
                            break;
                        }
                    }
                }
                debug!(?index, "self source sensed");
                let msg = format!("I know that this is from my own source code:\n{}", chunk);
                let s = Sensation {
                    kind: "self_source".into(),
                    when: chrono::Local::now(),
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
        super::set_fast_test_env();
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

    #[test]
    fn includes_readme() {
        assert!(CHUNKS.iter().any(|c| c.contains("Pete Daringsby")));
    }
}
