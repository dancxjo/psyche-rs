use async_trait::async_trait;

/// A cognitive distillation unit transforming streams of input memories into
/// distilled outputs.
#[async_trait(?Send)]
pub trait Wit<TInput, TOutput> {
    /// Observe a new input event.
    async fn observe(&mut self, input: TInput);

    /// Attempt to produce a distilled output from previously observed inputs.
    async fn distill(&mut self) -> Option<TOutput>;
}
