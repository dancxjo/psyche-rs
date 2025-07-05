use super::genius::Genius;
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{debug, trace};

/// Basic input type describing the latest instant for [`QuickGenius`].
#[derive(Debug, Clone)]
pub struct InstantInput {
    /// Free form description of what just happened.
    pub description: String,
}

/// Output from [`QuickGenius`] summarizing the instant.
#[derive(Debug, Clone, PartialEq)]
pub struct InstantOutput {
    pub description: String,
}

/// Example genius that turns [`InstantInput`]s into short summaries using an LLM.
pub struct QuickGenius {
    input_rx: Mutex<Option<UnboundedReceiver<InstantInput>>>,
    output_tx: UnboundedSender<InstantOutput>,
}

impl QuickGenius {
    /// Create a new [`QuickGenius`] with the provided channels.
    pub fn new(
        input_rx: UnboundedReceiver<InstantInput>,
        output_tx: UnboundedSender<InstantOutput>,
    ) -> Self {
        Self {
            input_rx: Mutex::new(Some(input_rx)),
            output_tx,
        }
    }

    async fn generate_prompt(&self, input: &InstantInput) -> String {
        format!("Describe this instant: {}", input.description)
    }

    async fn call_llm(&self, prompt: String) -> InstantOutput {
        trace!(%prompt, "quick_prompt");
        // Stub LLM call. In real code, send `prompt` to an LLM and await the result.
        let reply = prompt;
        debug!(reply, "quick_llm_reply");
        InstantOutput { description: reply }
    }
}

#[async_trait]
impl Genius for QuickGenius {
    type Input = InstantInput;
    type Output = InstantOutput;

    fn name(&self) -> &'static str {
        "Quick"
    }

    async fn call(&self, input: Self::Input) -> crate::llm::types::TokenStream {
        use crate::llm::types::Token;
        use futures::stream;
        let prompt = self.generate_prompt(&input).await;
        let out = self.call_llm(prompt).await;
        stream::once(async move {
            Token {
                text: out.description,
            }
        })
        .boxed()
    }

    async fn run(&self) {
        let mut rx = self
            .input_rx
            .lock()
            .unwrap()
            .take()
            .expect("run called twice");
        while let Some(input) = rx.recv().await {
            let prompt = self.generate_prompt(&input).await;
            let output = self.call_llm(prompt).await;
            let _ = self.output_tx.send(output);
        }
    }
}

/// # Orchestrating a [`QuickGenius`]
///
/// ```no_run
/// use psyche_rs::{genius::Genius, quick::{QuickGenius, InstantInput}};
/// use tokio::sync::mpsc::unbounded_channel;
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() {
///     let (in_tx, in_rx) = unbounded_channel();
///     let (out_tx, mut out_rx) = unbounded_channel();
///     let quick = Arc::new(QuickGenius::new(in_rx, out_tx));
///     tokio::spawn({
///         let quick = Arc::clone(&quick);
///         async move { quick.run().await }
///     });
///     in_tx.send(InstantInput { description: "ping".into() }).unwrap();
///     if let Some(out) = out_rx.recv().await {
///         println!("{}", out.description);
///     }
/// }
/// ```

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn produces_output() {
        let (tx, rx) = unbounded_channel();
        let (out_tx, mut out_rx) = unbounded_channel();
        let genius = QuickGenius::new(rx, out_tx);
        let handle = tokio::spawn(async move { genius.run().await });
        tx.send(InstantInput {
            description: "foo".into(),
        })
        .unwrap();
        let out = out_rx.recv().await.unwrap();
        assert!(out.description.contains("foo"));
        handle.abort();
    }
}
