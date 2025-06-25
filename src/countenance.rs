/// Traits for reflecting Pete's emotional state externally.
///
/// Implementations may update a UI, light pattern, or simply log to the
/// console.
///
/// # Example
///
/// ```
/// use psyche_rs::Countenance;
///
/// struct DummyFace;
/// impl Countenance for DummyFace {
///     fn reflect(&self, mood: &str) {
///         println!("Current mood: {}", mood);
///     }
/// }
/// ```
pub trait Countenance: Send + Sync {
    /// Reflect the provided mood string.
    fn reflect(&self, mood: &str);
}
