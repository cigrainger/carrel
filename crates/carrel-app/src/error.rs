//! Error boundary for Tauri command serialization.

use serde::Serialize;

/// Application result alias.
pub type Result<T> = std::result::Result<T, AppError>;

/// Errors raised by the desktop shell.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Explicit article capture failed before it could be archived.
    #[error("{0}")]
    Capture(String),
    /// Explicit desktop pairing or private synchronization failed.
    #[error(transparent)]
    Sync(#[from] carrel_sync::SyncError),
    /// Local configuration could not be resolved.
    #[error("{0}")]
    Config(String),

    /// The store failed to open, migrate, or answer a query.
    #[error(transparent)]
    Store(#[from] carrel_store::StoreError),

    /// The blob store failed to read cached content.
    #[error(transparent)]
    Blob(#[from] carrel_store::blobs::BlobError),

    /// The local device key could not be loaded.
    #[error(transparent)]
    Keystore(#[from] carrel_store::keystore::KeystoreError),

    /// Feed subscription or fetch orchestration failed.
    #[error("{0}")]
    Feed(String),

    /// Subscription interchange could not read, validate, or save a file.
    #[error("{0}")]
    SubscriptionFile(String),

    /// Annotation export could not save a file.
    #[error("{0}")]
    AnnotationFile(String),

    /// Tauri failed while building or running the shell.
    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    /// The operating system could not open an article link.
    #[error(transparent)]
    OpenLink(#[from] tauri_plugin_opener::Error),

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
            Self::Capture(_) => "capture",
            Self::Sync(_) => "sync",
            Self::Config(_) => "config",
            Self::Store(carrel_store::StoreError::AnnotationConflict { .. }) => {
                "annotation_conflict"
            }
            Self::Store(_) => "store",
            Self::Blob(_) => "blob",
            Self::Keystore(_) => "keystore",
            Self::Feed(_) => "feed",
            Self::SubscriptionFile(_) => "subscription_file",
            Self::AnnotationFile(_) => "annotation_file",
            Self::Tauri(_) => "tauri",
            Self::OpenLink(_) => "open_link",
            Self::InvalidData { .. } => "invalid_data",
        };

        SerializedAppError {
            kind,
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}
