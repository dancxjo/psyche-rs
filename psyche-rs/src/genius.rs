use async_trait::async_trait;

/// Trait implemented by autonomous sub-agents running as independent tasks.
#[async_trait]
pub trait Genius: Send + Sync + 'static {
    /// Input type handled by this genius.
    type Input: Send + Sync + 'static;
    /// Output type produced by this genius.
    type Output: Send + Sync + 'static;

    /// Human readable name used for logging.
    fn name(&self) -> &'static str;

    /// Feed an input into this genius.
    async fn feed(&self, input: Self::Input);

    /// Run the main loop for this genius.
    async fn run(&self);
}
