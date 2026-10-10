//! Content-addressed blob storage for readable article bodies and assets.

use std::fmt;
use std::io::Write;
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
            match self.get_blocking(&id) {
                Ok(_) => return Ok(id),
                // An interrupted older write or corruption is repaired only
                // with bytes whose digest proves this exact content identity.
                Err(BlobError::IntegrityMismatch { .. } | BlobError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }

        let Some(parent) = path.parent() else {
            return Err(BlobError::InvalidPath(path));
        };
        std::fs::create_dir_all(parent).map_err(|source| BlobError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
        let write_error = |source| BlobError::Write {
            path: path.clone(),
            source,
        };
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(write_error)?;
        temporary.write_all(bytes).map_err(write_error)?;
        temporary.as_file().sync_all().map_err(write_error)?;
        temporary
            .persist(&path)
            .map_err(|error| write_error(error.error))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(write_error)?;

        Ok(id)
    }

    /// Load blob bytes by id.
    pub async fn get(&self, id: &BlobId) -> Result<Bytes, BlobError> {
        let path = self.blob_path(id);
        let data = tokio::fs::read(&path)
            .await
            .map(Bytes::from)
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::NotFound {
                    BlobError::NotFound { id: *id }
                } else {
                    BlobError::Read { path, source }
                }
            })?;
        verify(id, data)
    }

    /// Load blob bytes by id from synchronous callers.
    pub fn get_blocking(&self, id: &BlobId) -> Result<Bytes, BlobError> {
        let path = self.blob_path(id);
        let data = std::fs::read(&path).map(Bytes::from).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                BlobError::NotFound { id: *id }
            } else {
                BlobError::Read { path, source }
            }
        })?;
        verify(id, data)
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
    /// Bytes at the content address do not match its digest.
    #[error("cached blob {id} failed integrity verification")]
    IntegrityMismatch {
        /// Content address being verified.
        id: BlobId,
    },
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

fn verify(id: &BlobId, data: Bytes) -> Result<Bytes, BlobError> {
    if BlobId::from_bytes(&data) != *id {
        return Err(BlobError::IntegrityMismatch { id: *id });
    }
    Ok(data)
}

#[cfg(test)]
mod integrity_tests {
    use super::*;

    #[tokio::test]
    async fn corruption_is_rejected_on_both_read_paths_and_exact_content_repairs_it() {
        let temp = tempfile::tempdir().unwrap();
        let store = BlobStore::open(temp.path());
        let original = b"Original caf\xc3\xa9 content";
        let id = store.put_blocking(original).unwrap();
        std::fs::write(store.blob_path(&id), b"truncated").unwrap();
        assert!(matches!(
            store.get_blocking(&id),
            Err(BlobError::IntegrityMismatch { .. })
        ));
        assert!(matches!(
            store.get(&id).await,
            Err(BlobError::IntegrityMismatch { .. })
        ));
        assert_eq!(store.put_blocking(original).unwrap(), id);
        assert_eq!(store.get(&id).await.unwrap().as_ref(), original);
    }

    #[test]
    fn concurrent_atomic_writes_never_expose_partial_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = BlobStore::open(temp.path());
        let data = vec![b'x'; 512 * 1024];
        let id = BlobId::from_bytes(&data);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let store = store.clone();
                let data = &data;
                scope.spawn(move || {
                    for _ in 0..8 {
                        assert_eq!(store.put_blocking(data).unwrap(), id);
                    }
                });
            }
            let store = store.clone();
            let data = &data;
            scope.spawn(move || {
                for _ in 0..64 {
                    match store.get_blocking(&id) {
                        Ok(bytes) => assert_eq!(bytes.as_ref(), data),
                        Err(BlobError::NotFound { .. }) => {}
                        Err(error) => panic!("partial or invalid atomic write: {error}"),
                    }
                }
            });
        });
        assert_eq!(store.get_blocking(&id).unwrap().as_ref(), data);
    }
}
