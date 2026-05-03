//! Error types for the store boundary.

use std::io;
use std::path::PathBuf;

/// Store-layer result alias.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Errors raised by the Carrel store boundary.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Could not create the directory backing the persistent store.
    #[error("failed to create store directory {path}")]
    CreateStoreDir {
        /// Directory that could not be created.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// Cozo returned an error while opening, migrating, or querying the store.
    #[error("cozo error: {0}")]
    Cozo(String),

    /// A URL was malformed or used an unsupported scheme.
    #[error("invalid URL {url}: {source}")]
    InvalidUrl {
        /// URL value being validated.
        url: String,
        /// Underlying parse error.
        #[source]
        source: url::ParseError,
    },

    /// The schema version relation contained an invalid version number.
    #[error("invalid schema version {0}")]
    InvalidSchemaVersion(i64),

    /// A query returned a value with an unexpected shape.
    #[error("unexpected value for {context}: {value}")]
    UnexpectedValue {
        /// What was being decoded.
        context: &'static str,
        /// Debug representation of the value.
        value: String,
    },
}

impl From<cozo::Error> for StoreError {
    fn from(value: cozo::Error) -> Self {
        Self::Cozo(value.to_string())
    }
}
