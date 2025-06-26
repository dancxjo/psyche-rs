use async_trait::async_trait;
use tokio::sync::{broadcast, mpsc};

/// A handle to a running [`Wit`] task.
///
/// `sender` accepts inputs for the wit while `receiver` yields distilled outputs
/// to downstream subscribers.
pub struct WitHandle<TIn, TOut> {
    pub sender: mpsc::Sender<TIn>,
    pub receiver: broadcast::Receiver<TOut>,
}

/// A cognitive distillation unit transforming streams of input memories into
/// distilled outputs.
#[async_trait(?Send)]
pub trait Wit<TInput: Send + 'static, TOutput: Clone + Send + 'static> {
    /// Observe a new input event.
    async fn observe(&mut self, input: TInput);

    /// Attempt to produce a distilled output from previously observed inputs.
    async fn distill(&mut self) -> Option<TOutput>;

    /// Run the wit as a reactive loop receiving inputs from `input` and
    /// publishing outputs via `output`.
    async fn run(mut self, mut input: mpsc::Receiver<TInput>, output: broadcast::Sender<TOutput>)
    where
        Self: Sized,
    {
        while let Some(i) = input.recv().await {
            self.observe(i).await;
            if let Some(out) = self.distill().await {
                let _ = output.send(out);
            }
        }
    }
}
