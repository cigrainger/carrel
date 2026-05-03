//! On-disk persistence for Carrel identity keys.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use argon2::Argon2;
use carrel_core::identity::{Keypair, SECRET_KEY_LENGTH};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const MASTER_KEY_FILE: &str = "master.enc";
const DEVICE_KEY_FILE: &str = "device.key";
const FORMAT_VERSION: u32 = 1;
const SALT_LENGTH: usize = 16;
const XCHACHA_NONCE_LENGTH: usize = 24;
const DERIVED_KEY_LENGTH: usize = 32;

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
    pub fn save_master(&self, keypair: &Keypair, passphrase: &str) -> Result<()> {
        fs::create_dir_all(&self.path).map_err(|source| KeystoreError::CreateKeystoreDir {
            path: self.path.clone(),
            source,
        })?;

        let mut salt = [0_u8; SALT_LENGTH];
        let mut nonce = [0_u8; XCHACHA_NONCE_LENGTH];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce);

        let key = derive_key(passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .expect("derived key is always 32 bytes");
        let seed = Zeroizing::new(keypair.to_seed_bytes());
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce), seed.as_ref())
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

        self.write_json(self.master_path(), &file)
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
        let seed = cipher
            .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| KeystoreError::WrongPassphrase)?;
        let seed = copy_fixed::<SECRET_KEY_LENGTH>("master seed", &seed)?;
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
        fs::create_dir_all(&self.path).map_err(|source| KeystoreError::CreateKeystoreDir {
            path: self.path.clone(),
            source,
        })?;

        let seed = Zeroizing::new(keypair.to_seed_bytes());
        let file = PlainDeviceKeyFile {
            version: FORMAT_VERSION,
            public_key: keypair.public().to_hex(),
            seed: hex::encode(seed.as_ref()),
        };

        self.write_json(self.device_path(), &file)
    }

    /// Load a plain device keypair.
    pub fn load_device(&self) -> Result<Keypair> {
        let path = self.device_path();
        let file: PlainDeviceKeyFile = self.read_json(&path)?;
        file.require_version(path.as_path())?;

        let seed = decode_hex::<SECRET_KEY_LENGTH>("device seed", &file.seed)?;
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
        let bytes = fs::read(path).map_err(|source| KeystoreError::ReadKeyFile {
            path: path.to_path_buf(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|source| KeystoreError::ParseKeyFile {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write_json<T>(&self, path: PathBuf, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec_pretty(value).map_err(KeystoreError::SerializeKeyFile)?;
        fs::write(&path, bytes).map_err(|source| KeystoreError::WriteKeyFile { path, source })
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

#[derive(Debug, Deserialize, Serialize)]
struct PlainDeviceKeyFile {
    version: u32,
    public_key: String,
    seed: String,
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
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|source| KeystoreError::DeriveKey(source.to_string()))?;
    Ok(key)
}

fn decode_hex<const N: usize>(field: &'static str, value: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(value).map_err(|source| KeystoreError::DecodeHex { field, source })?;
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
