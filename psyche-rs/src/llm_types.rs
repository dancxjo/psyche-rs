use futures::stream::BoxStream;

/// Text token emitted by an LLM.
#[derive(Debug, Clone)]
pub struct Token {
    /// Token text as provided by the model.
    pub text: String,
}

/// Stream of [`Token`] values.
pub type TokenStream = BoxStream<'static, Token>;
