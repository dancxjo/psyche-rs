use chrono::Local;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{Event, Level};
use tracing_subscriber::{
    layer::{Context, Layer},
    prelude::*,
    registry::LookupSpan,
};

use psyche_rs::Sensation;

/// Tracing layer that converts log events above `DEBUG` into `log.system`
/// sensations sent through the provided channel.
pub struct LogSensationLayer {
    tx: UnboundedSender<Vec<Sensation<String>>>,
}

impl LogSensationLayer {
    /// Create a new layer sending sensations via `tx`.
    pub fn new(tx: UnboundedSender<Vec<Sensation<String>>>) -> Self {
        Self { tx }
    }
}

struct FieldVisitor {
    message: String,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}

impl<S> Layer<S> for LogSensationLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() <= Level::DEBUG {
            return;
        }
        let mut visitor = FieldVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);
        if visitor.message.is_empty() {
            visitor.message = event.metadata().target().to_string();
        }
        let sensation = Sensation {
            kind: "log.system".into(),
            when: Local::now(),
            what: visitor.message,
            source: None,
        };
        let _ = self.tx.send(vec![sensation]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore]
    async fn forwards_info_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tracing_subscriber::registry()
            .with(LogSensationLayer::new(tx))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .unwrap();
        tracing::info!("hello world");
        tokio::task::yield_now().await;
        let batch = rx.try_recv().unwrap();
        assert_eq!(batch[0].what, "\"hello world\"");
    }

    #[tokio::test]
    #[ignore]
    async fn ignores_debug_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tracing_subscriber::registry()
            .with(LogSensationLayer::new(tx))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .unwrap();
        tracing::debug!("hi");
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err());
    }
}
