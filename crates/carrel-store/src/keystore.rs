//! On-disk persistence for Carrel identity keys.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use carrel_core::identity::{Keypair, PublicKey, SECRET_KEY_LENGTH};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const MASTER_KEY_FILE: &str = "master.enc";
const DEVICE_KEY_FILE: &str = "device.key";
const FORMAT_VERSION: u32 = 1;
const SALT_LENGTH: usize = 16;
const XCHACHA_NONCE_LENGTH: usize = 24;
const DERIVED_KEY_LENGTH: usize = 32;
const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;

/// Store-layer result alias for key persistence.
pub type Result<T> = std::result::Result<T, KeystoreError>;

/// Filesystem-backed key persistence.
#[derive(Clone, Debug)]
pub struct Keystore {
    path: PathBuf,
}

impl Keystore {
    /// Open a keystore rooted at `path`.
    pub fn open(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Save the master keypair encrypted with `passphrase`.
    ///
    /// Replaces an existing file atomically. Initial setup must use
    /// [`Self::create_master`] to avoid overwriting an existing identity.
    pub fn save_master(&self, keypair: &Keypair, passphrase: &str) -> Result<()> {
        self.write_master(keypair, passphrase, false)
    }

    /// Create an encrypted master file without replacing any existing file.
    pub fn create_master(&self, keypair: &Keypair, passphrase: &str) -> Result<()> {
        self.write_master(keypair, passphrase, true)
    }

    /// Copy only the already-encrypted recovery key into a private backup.
    pub fn encrypted_master(&self) -> Result<Option<Vec<u8>>> {
        match self.master_public_key() {
            Ok(_) => self
                .read_json::<EncryptedMasterKeyFile>(&self.master_path())
                .and_then(|file| serde_json::to_vec(&file).map_err(KeystoreError::SerializeKeyFile))
                .map(Some),
            Err(KeystoreError::ReadKeyFile { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// An empty installation may discard its unused device key, never other key files.
    pub fn contains_only_device_key(&self) -> Result<bool> {
        let entries = match fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(source) => {
                return Err(KeystoreError::ReadKeyFile {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| KeystoreError::ReadKeyFile {
                path: self.path.clone(),
                source,
            })?;
            let kind = entry
                .file_type()
                .map_err(|source| KeystoreError::ReadKeyFile {
                    path: entry.path(),
                    source,
                })?;
            if entry.file_name() != DEVICE_KEY_FILE || !kind.is_file() {
                return Ok(false);
            }
            self.load_device()?;
        }
        Ok(true)
    }

    /// Install a bounded encrypted master from a validated backup, without unlocking it.
    pub fn import_encrypted_master(&self, bytes: &[u8]) -> Result<PublicKey> {
        // Use the existing bounded reader and format validator, then create rather
        // than replace. The staging directory is owned by the recovery operation.
        if bytes.len() > MAX_KEY_FILE_BYTES as usize {
            return Err(KeystoreError::KeyFileTooLarge {
                path: self.master_path(),
            });
        }
        let file: EncryptedMasterKeyFile =
            serde_json::from_slice(bytes).map_err(|source| KeystoreError::ParseKeyFile {
                path: self.master_path(),
                source,
            })?;
        file.require_version(&self.master_path())?;
        file.require_algorithm(&self.master_path())?;
        decode_hex::<SALT_LENGTH>("master salt", &file.salt)?;
        decode_hex::<XCHACHA_NONCE_LENGTH>("master nonce", &file.nonce)?;
        decode_hex::<48>("encrypted master seed", &file.ciphertext)?;
        let public = PublicKey::from_bytes(decode_hex("master public key", &file.public_key)?);
        public
            .validate()
            .map_err(|_| KeystoreError::InvalidPublicKey {
                path: self.master_path(),
            })?;
        self.write_json(self.master_path(), &file, true)?;
        Ok(public)
    }

    fn write_master(&self, keypair: &Keypair, passphrase: &str, create: bool) -> Result<()> {
        let mut salt = [0_u8; SALT_LENGTH];
        let mut nonce = [0_u8; XCHACHA_NONCE_LENGTH];
        getrandom::fill(&mut salt).map_err(KeystoreError::Randomness)?;
        getrandom::fill(&mut nonce).map_err(KeystoreError::Randomness)?;

        let key = derive_key(passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .expect("derived key is always 32 bytes");
        let seed = Zeroizing::new(keypair.to_seed_bytes());
        let ciphertext = cipher
            .encrypt(&XNonce::from(nonce), seed.as_ref())
            .map_err(|_| KeystoreError::EncryptMaster)?;

        let file = EncryptedMasterKeyFile {
            version: FORMAT_VERSION,
            kdf: "argon2id-default".to_string(),
            cipher: "xchacha20poly1305".to_string(),
            public_key: keypair.public().to_hex(),
            salt: hex::encode(salt),
            nonce: hex::encode(nonce),
            ciphertext: hex::encode(ciphertext),
        };

        self.write_json(self.master_path(), &file, create)
    }

    /// Read the encrypted master's public header without unlocking the seed.
    ///
    /// This identifies a local recovery file; it does not establish trust in a
    /// remote device. Certificates must still be verified against the binding.
    pub fn master_public_key(&self) -> Result<PublicKey> {
        let path = self.master_path();
        let file: EncryptedMasterKeyFile = self.read_json(&path)?;
        file.require_version(&path)?;
        file.require_algorithm(&path)?;
        let key = PublicKey::from_bytes(decode_hex("master public key", &file.public_key)?);
        key.validate()
            .map_err(|_| KeystoreError::InvalidPublicKey { path })?;
        Ok(key)
    }

    /// Load and decrypt the master keypair with `passphrase`.
    pub fn load_master(&self, passphrase: &str) -> Result<Keypair> {
        let path = self.master_path();
        let file: EncryptedMasterKeyFile = self.read_json(&path)?;
        file.require_version(path.as_path())?;
        file.require_algorithm(path.as_path())?;

        let salt = decode_hex::<SALT_LENGTH>("salt", &file.salt)?;
        let nonce = decode_hex::<XCHACHA_NONCE_LENGTH>("nonce", &file.nonce)?;
        let ciphertext =
            hex::decode(&file.ciphertext).map_err(|source| KeystoreError::DecodeHex {
                field: "ciphertext",
                source,
            })?;

        let key = derive_key(passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .expect("derived key is always 32 bytes");
        let seed = Zeroizing::new(
            cipher
                .decrypt(&XNonce::from(nonce), ciphertext.as_ref())
                .map_err(|_| KeystoreError::WrongPassphrase)?,
        );
        let seed = Zeroizing::new(copy_fixed::<SECRET_KEY_LENGTH>("master seed", &seed)?);
        let keypair = Keypair::from_seed(&seed);

        if keypair.public().to_hex() != file.public_key {
            return Err(KeystoreError::KeyMaterialMismatch {
                path,
                key_kind: "master",
            });
        }

        Ok(keypair)
    }

    /// Save a device keypair in plain JSON.
    ///
    /// Device keys are intentionally plain for v1: the user's device storage is
    /// the trust boundary, while the master key remains passphrase-encrypted.
    pub fn save_device(&self, keypair: &Keypair) -> Result<()> {
        self.write_device(keypair, false)
    }

    /// Create a device key file without replacing any existing file.
    pub fn create_device(&self, keypair: &Keypair) -> Result<()> {
        self.write_device(keypair, true)
    }

    fn write_device(&self, keypair: &Keypair, create: bool) -> Result<()> {
        let seed = Zeroizing::new(keypair.to_seed_bytes());
        let file = PlainDeviceKeyFile {
            version: FORMAT_VERSION,
            public_key: keypair.public().to_hex(),
            seed: hex::encode(&seed[..]),
        };

        self.write_json(self.device_path(), &file, create)
    }

    /// Load a plain device keypair.
    pub fn load_device(&self) -> Result<Keypair> {
        let path = self.device_path();
        let file: PlainDeviceKeyFile = self.read_json(&path)?;
        file.require_version(path.as_path())?;

        let seed = Zeroizing::new(decode_hex::<SECRET_KEY_LENGTH>("device seed", &file.seed)?);
        let keypair = Keypair::from_seed(&seed);

        if keypair.public().to_hex() != file.public_key {
            return Err(KeystoreError::KeyMaterialMismatch {
                path,
                key_kind: "device",
            });
        }

        Ok(keypair)
    }

    fn master_path(&self) -> PathBuf {
        self.path.join(MASTER_KEY_FILE)
    }

    fn device_path(&self) -> PathBuf {
        self.path.join(DEVICE_KEY_FILE)
    }

    fn read_json<T>(&self, path: &Path) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let read_error = |source| KeystoreError::ReadKeyFile {
            path: path.to_path_buf(),
            source,
        };
        let file = fs::File::open(path).map_err(read_error)?;
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(MAX_KEY_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        if bytes.len() as u64 > MAX_KEY_FILE_BYTES {
            return Err(KeystoreError::KeyFileTooLarge {
                path: path.to_path_buf(),
            });
        }
        serde_json::from_slice(&bytes).map_err(|source| KeystoreError::ParseKeyFile {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write_json<T>(&self, path: PathBuf, value: &T, create: bool) -> Result<()>
    where
        T: Serialize,
    {
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(value).map_err(KeystoreError::SerializeKeyFile)?,
        );
        fs::create_dir_all(&self.path).map_err(|source| KeystoreError::CreateKeystoreDir {
            path: self.path.clone(),
            source,
        })?;
        let write_error = |source| KeystoreError::WriteKeyFile {
            path: path.clone(),
            source,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700))
                .map_err(write_error)?;
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.path).map_err(write_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(write_error)?;
        }
        file.write_all(&bytes).map_err(write_error)?;
        file.as_file().sync_all().map_err(write_error)?;
        let installed = if create {
            file.persist_noclobber(&path)
        } else {
            file.persist(&path)
        };
        installed.map_err(|error| {
            if create && error.error.kind() == io::ErrorKind::AlreadyExists {
                KeystoreError::KeyAlreadyExists { path: path.clone() }
            } else {
                write_error(error.error)
            }
        })?;
        // Syncing the directory makes the installed name durable on Unix.
        #[cfg(unix)]
        fs::File::open(&self.path)
            .and_then(|dir| dir.sync_all())
            .map_err(write_error)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct EncryptedMasterKeyFile {
    version: u32,
    kdf: String,
    cipher: String,
    public_key: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

impl EncryptedMasterKeyFile {
    fn require_version(&self, path: &Path) -> Result<()> {
        if self.version == FORMAT_VERSION {
            Ok(())
        } else {
            Err(KeystoreError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: self.version,
            })
        }
    }

    fn require_algorithm(&self, path: &Path) -> Result<()> {
        if self.kdf == "argon2id-default" && self.cipher == "xchacha20poly1305" {
            Ok(())
        } else {
            Err(KeystoreError::UnsupportedAlgorithm {
                path: path.to_path_buf(),
                kdf: self.kdf.clone(),
                cipher: self.cipher.clone(),
            })
        }
    }
}

#[derive(Deserialize, Serialize)]
struct PlainDeviceKeyFile {
    version: u32,
    public_key: String,
    seed: String,
}

impl Drop for PlainDeviceKeyFile {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl PlainDeviceKeyFile {
    fn require_version(&self, path: &Path) -> Result<()> {
        if self.version == FORMAT_VERSION {
            Ok(())
        } else {
            Err(KeystoreError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: self.version,
            })
        }
    }
}

fn derive_key(passphrase: &str, salt: &[u8; SALT_LENGTH]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0_u8; DERIVED_KEY_LENGTH]);
    // Format v1 used the argon2 0.5.3 defaults. Freeze them so library updates
    // cannot change the derived key for an existing encrypted identity.
    let params = Params::new(19_456, 2, 1, Some(DERIVED_KEY_LENGTH))
        .expect("format v1 Argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|source| KeystoreError::DeriveKey(source.to_string()))?;
    Ok(key)
}

fn decode_hex<const N: usize>(field: &'static str, value: &str) -> Result<[u8; N]> {
    let bytes = Zeroizing::new(
        hex::decode(value).map_err(|source| KeystoreError::DecodeHex { field, source })?,
    );
    copy_fixed(field, &bytes)
}

fn copy_fixed<const N: usize>(field: &'static str, bytes: &[u8]) -> Result<[u8; N]> {
    bytes
        .try_into()
        .map_err(|_| KeystoreError::InvalidKeyLength {
            field,
            expected: N,
            actual: bytes.len(),
        })
}

/// Errors raised while saving or loading identity keys.
#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    /// Initial setup found an existing key and refused to replace it.
    #[error("key file already exists at {path}")]
    KeyAlreadyExists {
        /// Existing file path.
        path: PathBuf,
    },
    /// A key file exceeds the bounded local format size.
    #[error("key file exceeds {MAX_KEY_FILE_BYTES} bytes at {path}")]
    KeyFileTooLarge {
        /// Oversized file path.
        path: PathBuf,
    },
    /// The public header is not a usable Ed25519 key.
    #[error("invalid public key in {path}")]
    InvalidPublicKey {
        /// Key file path.
        path: PathBuf,
    },
    /// Operating-system randomness could not be read for a salt or nonce.
    #[error("operating-system randomness is unavailable")]
    Randomness(#[source] getrandom::Error),

    /// The keystore directory could not be created.
    #[error("failed to create keystore directory {path}")]
    CreateKeystoreDir {
        /// Directory that could not be created.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// A key file could not be read.
    #[error("failed to read key file {path}")]
    ReadKeyFile {
        /// Key file path.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// A key file could not be written.
    #[error("failed to write key file {path}")]
    WriteKeyFile {
        /// Key file path.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// A key file could not be parsed as JSON.
    #[error("failed to parse key file {path}")]
    ParseKeyFile {
        /// Key file path.
        path: PathBuf,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// A key file could not be serialized as JSON.
    #[error("failed to serialize key file")]
    SerializeKeyFile(#[source] serde_json::Error),

    /// The key file format version is not supported.
    #[error("unsupported key file version {version} in {path}")]
    UnsupportedVersion {
        /// Key file path.
        path: PathBuf,
        /// Unsupported version value.
        version: u32,
    },

    /// The key file names algorithms this build does not support.
    #[error("unsupported key file algorithms in {path}: kdf={kdf}, cipher={cipher}")]
    UnsupportedAlgorithm {
        /// Key file path.
        path: PathBuf,
        /// Key derivation function identifier.
        kdf: String,
        /// Cipher identifier.
        cipher: String,
    },

    /// Argon2id failed while deriving an encryption key.
    #[error("failed to derive encryption key: {0}")]
    DeriveKey(String),

    /// Master-key encryption failed.
    #[error("failed to encrypt master key")]
    EncryptMaster,

    /// The passphrase did not decrypt the master key.
    #[error("wrong passphrase for master key")]
    WrongPassphrase,

    /// A hex-encoded field in the key file could not be decoded.
    #[error("invalid hex in {field}")]
    DecodeHex {
        /// Field name.
        field: &'static str,
        /// Underlying hex decoder error.
        #[source]
        source: hex::FromHexError,
    },

    /// Decoded key material had the wrong length.
    #[error("invalid length for {field}: expected {expected} bytes, got {actual}")]
    InvalidKeyLength {
        /// Field name.
        field: &'static str,
        /// Expected byte length.
        expected: usize,
        /// Actual byte length.
        actual: usize,
    },

    /// The public key stored beside the seed does not match the derived key.
    #[error("{key_kind} key material in {path} does not match its public key")]
    KeyMaterialMismatch {
        /// Key file path.
        path: PathBuf,
        /// Human-readable key kind.
        key_kind: &'static str,
    },
}
