use async_trait::async_trait;

/// Abstraction for outputting spoken text.
///
/// Implementations may route text to a TTS engine, a console log or any other
/// speaking mechanism.
///
/// # Example
///
/// ```no_run
/// use psyche_rs::mouth::{Mouth};
/// use async_trait::async_trait;
///
/// struct ConsoleMouth;
///
/// #[async_trait(?Send)]
/// impl Mouth for ConsoleMouth {
///     async fn say(&self, phrase: &str) -> anyhow::Result<()> {
///         println!("{}", phrase);
///         Ok(())
///     }
/// }
/// ```
#[async_trait(?Send)]
pub trait Mouth: Send + Sync {
    /// Speak the provided phrase.
    async fn say(&self, phrase: &str) -> anyhow::Result<()>;
}
