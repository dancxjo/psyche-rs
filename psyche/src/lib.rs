//! Core cognitive logic for Psyche.
//!
//! This crate hosts memory models and simple distillers. It is OS neutral.

pub mod distiller;
pub mod models;

/// Example helper that adds two numbers.
///
/// ```
/// use psyche::add;
/// assert_eq!(add(2, 2), 4);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
