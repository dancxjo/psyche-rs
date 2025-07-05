use crate::llm::types::TokenStream;
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

    /// Process an input and stream tokens back.
    async fn call(&self, input: Self::Input) -> TokenStream;

    /// Run the main loop for this genius.
    async fn run(&self);
}
