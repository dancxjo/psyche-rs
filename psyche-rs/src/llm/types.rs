use futures::stream::BoxStream;

/// Token emitted by language model streams.
#[derive(Debug, Clone)]
pub struct Token {
    /// Raw text for the token.
    pub text: String,
    // Additional metadata can be added here (e.g. logprobs, index).
}

/// Boxed stream of [`Token`] values produced asynchronously.
///
/// This stream is `Send` and `Sync`, allowing it to be passed between tasks.
pub type TokenStream = BoxStream<'static, Token>;
