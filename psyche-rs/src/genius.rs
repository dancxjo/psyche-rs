use async_trait::async_trait;

/// Trait implemented by autonomous sub-agents running as independent tasks.
#[async_trait]
pub trait Genius: Send + Sync + 'static {
    /// Input type handled by this genius.
    type Input: Send + Sync + 'static;

    /// Human readable name used for logging.
    fn name(&self) -> &'static str;

    /// Invoke this genius with the provided input returning a stream of tokens.
    async fn call(&self, input: Self::Input) -> crate::TokenStream;

    /// Run the main loop for this genius.
    async fn run(&self);
}
