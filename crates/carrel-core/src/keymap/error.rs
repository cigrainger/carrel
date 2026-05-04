//! Keymap error types.

/// Error raised while parsing or registering keymap data.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum KeymapError {
    /// A key string could not be parsed.
    #[error("invalid key `{input}`: {reason}")]
    InvalidKey {
        /// Original key text.
        input: String,
        /// Human-readable reason.
        reason: String,
    },

    /// A key sequence was empty.
    #[error("key sequence cannot be empty")]
    EmptySequence,

    /// A binding conflicts with another binding in the same layer.
    #[error("conflicting bindings in layer `{layer}`: `{first}` conflicts with `{second}`")]
    ConflictingBinding {
        /// Stable layer key.
        layer: String,
        /// First sequence.
        first: String,
        /// Second sequence.
        second: String,
    },
}
