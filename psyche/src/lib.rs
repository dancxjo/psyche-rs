//! Core cognitive logic for Psyche.
//!
//! This crate will host memory stores, wits and distillers. It is OS neutral.

/// Example helper that adds two numbers.
///
/// ```
/// use psyche::add;
/// assert_eq!(add(2, 2), 4);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
