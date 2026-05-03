//! CLI error types and exit-code mapping.

use std::io;
use std::path::PathBuf;

/// CLI result alias.
pub type Result<T> = std::result::Result<T, CliError>;

/// Errors raised by the CLI boundary.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The user asked for an operation that cannot be performed as requested.
    #[error("{0}")]
    User(String),

    /// An unexpected internal failure occurred.
    #[error("{0}")]
    Internal(String),

    /// A filesystem operation failed.
    #[error("filesystem error for {path}: {source}")]
    Io {
        /// Path being read or written.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// Store-layer operation failed.
    #[error(transparent)]
    Store(#[from] carrel_store::StoreError),

    /// Feed fetching or parsing failed.
    #[error(transparent)]
    Fetch(#[from] carrel_feeds::FetchError),

    /// Feed parsing failed.
    #[error(transparent)]
    Parse(#[from] carrel_feeds::ParseError),

    /// Key persistence failed.
    #[error(transparent)]
    Keystore(#[from] carrel_store::keystore::KeystoreError),

    /// JSON serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Terminal prompt failed.
    #[error("failed to read terminal input: {0}")]
    Prompt(#[from] dialoguer::Error),
}

impl CliError {
    /// Construct a user-facing usage or environment error.
    pub fn user(message: impl Into<String>) -> Self {
        Self::User(message.into())
    }

    /// Construct an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Construct a path-aware filesystem error.
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Return the process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::User(_) | Self::Prompt(_) => 1,
            Self::Keystore(carrel_store::keystore::KeystoreError::WrongPassphrase) => 1,
            Self::Store(carrel_store::StoreError::InvalidUrl { .. }) => 1,
            Self::Io { source, .. } if source.kind() == io::ErrorKind::NotFound => 1,
            Self::Internal(_)
            | Self::Io { .. }
            | Self::Store(_)
            | Self::Fetch(_)
            | Self::Parse(_)
            | Self::Keystore(_)
            | Self::Json(_) => 2,
        }
    }
}
