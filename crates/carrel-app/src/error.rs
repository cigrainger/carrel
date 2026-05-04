//! Error boundary for Tauri command serialization.

use serde::Serialize;

/// Application result alias.
pub type Result<T> = std::result::Result<T, AppError>;

/// Errors raised by the desktop shell.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Local configuration could not be resolved.
    #[error("{0}")]
    Config(String),

    /// The store failed to open, migrate, or answer a query.
    #[error(transparent)]
    Store(#[from] carrel_store::StoreError),

    /// Tauri failed while building or running the shell.
    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    /// A Tauri command received or produced malformed data.
    #[error("invalid app data for {context}: {value}")]
    InvalidData {
        /// Context where the invalid data appeared.
        context: &'static str,
        /// Debug representation of the invalid value.
        value: String,
    },
}

#[derive(Serialize)]
struct SerializedAppError {
    kind: &'static str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let kind = match self {
            Self::Config(_) => "config",
            Self::Store(_) => "store",
            Self::Tauri(_) => "tauri",
            Self::InvalidData { .. } => "invalid_data",
        };

        SerializedAppError {
            kind,
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}
