//! Content-addressed blob storage for readable article bodies and assets.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use bytes::Bytes;

/// Filesystem-backed content-addressed blob store.
#[derive(Clone, Debug)]
pub struct BlobStore {
    path: PathBuf,
}

impl BlobStore {
    /// Open a blob store rooted at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Store bytes content-addressably and return their BLAKE3 blob id.
    pub async fn put(&self, bytes: &[u8]) -> Result<BlobId, BlobError> {
        self.put_blocking(bytes)
    }

    /// Store bytes content-addressably from synchronous callers.
    pub fn put_blocking(&self, bytes: &[u8]) -> Result<BlobId, BlobError> {
        let id = BlobId::from_bytes(bytes);
        let path = self.blob_path(&id);
        if path.exists() {
            return Ok(id);
        }

        let Some(parent) = path.parent() else {
            return Err(BlobError::InvalidPath(path));
        };
        std::fs::create_dir_all(parent).map_err(|source| BlobError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
        std::fs::write(&path, bytes).map_err(|source| BlobError::Write {
            path: path.clone(),
            source,
        })?;

        Ok(id)
    }

    /// Load blob bytes by id.
    pub async fn get(&self, id: &BlobId) -> Result<Bytes, BlobError> {
        let path = self.blob_path(id);
        tokio::fs::read(&path)
            .await
            .map(Bytes::from)
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::NotFound {
                    BlobError::NotFound { id: *id }
                } else {
                    BlobError::Read { path, source }
                }
            })
    }

    /// Return true if the blob exists locally.
    pub fn has(&self, id: &BlobId) -> bool {
        self.blob_path(id).exists()
    }

    fn blob_path(&self, id: &BlobId) -> PathBuf {
        let hex = id.to_string();
        self.path.join(&hex[0..2]).join(hex)
    }
}

/// A BLAKE3 content hash identifying a blob.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlobId(pub [u8; 32]);

impl BlobId {
    /// Hash bytes into a blob id.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Return the raw 32-byte id.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl FromStr for BlobId {
    type Err = BlobError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(value).map_err(|source| BlobError::InvalidId {
            value: value.to_string(),
            source,
        })?;
        let bytes: [u8; 32] =
            bytes
                .try_into()
                .map_err(|bytes: Vec<u8>| BlobError::InvalidIdLength {
                    value: value.to_string(),
                    len: bytes.len(),
                })?;
        Ok(Self(bytes))
    }
}

/// Blob storage errors.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// Blob id was not valid hex.
    #[error("invalid blob id {value}: {source}")]
    InvalidId {
        /// User-supplied blob id.
        value: String,
        /// Hex decode error.
        #[source]
        source: hex::FromHexError,
    },

    /// Blob id had the wrong byte length.
    #[error("invalid blob id {value}: expected 32 bytes, got {len}")]
    InvalidIdLength {
        /// User-supplied blob id.
        value: String,
        /// Decoded byte length.
        len: usize,
    },

    /// A derived blob path was invalid.
    #[error("invalid blob path {}", .0.display())]
    InvalidPath(PathBuf),

    /// Could not create a blob directory.
    #[error("failed to create blob directory {path}")]
    CreateDir {
        /// Directory being created.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    /// Could not write a blob file.
    #[error("failed to write blob {path}")]
    Write {
        /// Blob file being written.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    /// Could not read a blob file.
    #[error("failed to read blob {path}")]
    Read {
        /// Blob file being read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    /// The requested blob is not stored locally.
    #[error("blob {id} not found")]
    NotFound {
        /// Missing blob id.
        id: BlobId,
    },
}
