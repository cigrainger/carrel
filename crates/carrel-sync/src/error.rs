//! Typed errors at the private synchronization boundary.

/// A private transport operation failed.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Invalid social proof; private self facts are not social wire variants.
    #[error(transparent)]
    Share(#[from] carrel_core::sharing::ShareError),
    /// An invitation is malformed, expired, consumed, or fails its Root proof.
    #[error("invalid or expired pairing invitation")]
    InvalidInvitation,
    /// Invalid original private fact bytes or proof.
    #[error(transparent)]
    Fact(#[from] carrel_core::private_library::LibraryFactError),
    /// Reading or atomically installing the native cache failed.
    #[error(transparent)]
    Blob(#[from] carrel_store::blobs::BlobError),
    /// A private SDK header, key, or portable content identity is invalid.
    #[error("invalid private document entry")]
    InvalidEntry,
    /// Network or cached bytes disagree with the declared hash/length.
    #[error("private content failed hash or length verification")]
    BlobIntegrity,
    /// The source-of-truth store could not complete an operation.
    #[error(transparent)]
    Store(#[from] carrel_store::StoreError),
    /// Private transport cannot select an identity on the user's behalf.
    #[error("private transport requires an explicitly bound identity")]
    IdentityUnbound,
    /// The endpoint key must be this installation's certified operational key.
    #[error("private transport key differs from the bound device")]
    DeviceKeyMismatch,
    /// A blocked or revoked installation cannot expose private protocols.
    #[error("private transport device is not authorized")]
    DeviceUnauthorized,
    /// An SDK or worker operation failed, with context for its caller.
    #[error("{operation}: {message}")]
    Transport {
        /// Operation that failed.
        operation: &'static str,
        /// Underlying diagnostic, without fact bodies.
        message: String,
    },
}

/// Result of a private synchronization operation.
pub type Result<T> = std::result::Result<T, SyncError>;

pub(crate) fn transport_error(operation: &'static str, error: impl std::fmt::Display) -> SyncError {
    SyncError::Transport {
        operation,
        message: error.to_string(),
    }
}
