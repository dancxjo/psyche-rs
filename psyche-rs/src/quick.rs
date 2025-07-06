use super::genius::Genius;
use crate::genius_queue::{GeniusSender, bounded_channel};
use crate::memory_store::MemoryStore;
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{Receiver, UnboundedSender};
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
    input_rx: Mutex<Option<Receiver<InstantInput>>>,
    output_tx: UnboundedSender<InstantOutput>,
    store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    top_k: usize,
}

impl QuickGenius {
    /// Create a new [`QuickGenius`] with the provided bounded input receiver.
    pub fn new(
        input_rx: Receiver<InstantInput>,
        output_tx: UnboundedSender<InstantOutput>,
    ) -> Self {
        Self {
            input_rx: Mutex::new(Some(input_rx)),
            output_tx,
            store: None,
            top_k: 3,
        }
    }

    /// Construct a [`QuickGenius`] and input channel with the given capacity.
    pub fn with_capacity(
        capacity: usize,
        output_tx: UnboundedSender<InstantOutput>,
    ) -> (Self, GeniusSender<InstantInput>) {
        let (tx, rx) = bounded_channel(capacity, "Quick");
        (Self::new(rx, output_tx), tx)
    }

    /// Attach a memory store used to fetch neighbors for each instant.
    pub fn memory_store(mut self, store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the number of neighbors to retrieve when using a memory store.
    pub fn top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    async fn generate_prompt(&self, input: &InstantInput) -> String {
        let mut neighbors = String::new();
        if let Some(store) = &self.store {
            match store
                .retrieve_related_impressions(&input.description, self.top_k)
                .await
            {
                Ok(n) if !n.is_empty() => {
                    neighbors = n
                        .iter()
                        .map(|imp| format!("- {}", imp.how))
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(?e, "quick neighbor query failed"),
            }
        }
        if neighbors.is_empty() {
            format!("Describe this instant: {}", input.description)
        } else {
            format!(
                "Describe this instant: {}\nRelevant memories:\n{}",
                input.description, neighbors
            )
        }
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
///     let (out_tx, mut out_rx) = unbounded_channel();
///     let (quick, in_tx) = QuickGenius::with_capacity(4, out_tx);
///     let quick = Arc::new(quick);
///     tokio::spawn({
///         let quick = Arc::clone(&quick);
///         async move { quick.run().await }
///     });
///     in_tx.send(InstantInput { description: "ping".into() });
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
        let (out_tx, mut out_rx) = unbounded_channel();
        let (genius, tx) = QuickGenius::with_capacity(4, out_tx);
        let handle = tokio::spawn(async move { genius.run().await });
        tx.send(InstantInput {
            description: "foo".into(),
        });
        let out = out_rx.recv().await.unwrap();
        assert!(out.description.contains("foo"));
        handle.abort();
    }
}
