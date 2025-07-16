use serde::{Deserialize, Serialize};

/// Raw input received by the system.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Sensation {
    /// Unique identifier for the sensation.
    pub id: String,
    /// Path associated with the input, e.g. `/chat`.
    pub path: String,
    /// Text payload sent over the socket.
    pub text: String,
}

/// Simplified representation produced by the distiller.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Instant {
    /// Always `"instant"` for this basic vertical.
    pub kind: String,
    /// Human readable summary of the sensation.
    pub how: String,
    /// References to related sensations by id.
    pub what: Vec<String>,
}
