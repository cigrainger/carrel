//! Error types for the store boundary.

use std::io;
use std::path::PathBuf;

/// Store-layer result alias.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Errors raised by the Carrel store boundary.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A signed identity certificate failed verification or bounded decoding.
    #[error("invalid identity certificate: {0}")]
    IdentityCertificate(#[from] carrel_core::identity::CertError),
    /// The operation requires an explicitly chosen local self identity.
    #[error("self identity is not bound to this installation")]
    IdentityUnbound,
    /// An operation attempted to change the installation's chosen keys.
    #[error("self identity is already bound to different keys")]
    IdentityBindingConflict,
    /// A certificate was issued by a master other than the locally chosen one.
    #[error("certificate does not belong to the bound self identity")]
    IdentityMasterMismatch,
    /// A different certificate already exists for the same operational key.
    #[error("device certificate conflicts with the existing authorization")]
    IdentityCertificateConflict,
    /// The installation's device key has been revoked or blocked.
    #[error("device is not authorized under the bound self identity")]
    IdentityDeviceUnauthorized,
    /// An annotation or its target was removed or is missing.
    #[error("annotation is unavailable: {id}")]
    AnnotationUnavailable {
        /// Requested annotation identity.
        id: String,
    },
    /// A note changed after the editor opened its revision.
    #[error("note changed since editing began: {id}")]
    AnnotationConflict {
        /// Conflicting note identity.
        id: String,
    },
    /// A device cannot alter another author's annotation.
    #[error("device is not authorized to alter annotation: {id}")]
    AnnotationUnauthorized {
        /// Annotation identity.
        id: String,
    },
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

    /// Caller supplied malformed data for a typed store API.
    #[error("invalid input for {context}: {message}")]
    InvalidInput {
        /// API input or field being validated.
        context: &'static str,
        /// Human-readable validation failure.
        message: String,
    },

    /// The schema version relation contained an invalid version number.
    #[error("invalid schema version {0}")]
    InvalidSchemaVersion(i64),

    /// Canonical signing material could not be produced.
    #[error("failed to sign {context}: {message}")]
    Signing {
        /// Fact payload being signed.
        context: &'static str,
        /// Underlying signing failure.
        message: String,
    },

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
