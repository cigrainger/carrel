//! Ed25519 key and signature wrappers for Carrel identity.

use std::fmt;

use ed25519_dalek::{Signature as DalekSignature, Signer as _, SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

/// The length in bytes of an Ed25519 public key.
pub const PUBLIC_KEY_LENGTH: usize = 32;

/// The length in bytes of an Ed25519 seed secret.
pub const SECRET_KEY_LENGTH: usize = 32;

/// The length in bytes of an Ed25519 signature.
pub const SIGNATURE_LENGTH: usize = 64;

/// A Carrel Ed25519 public key.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicKey([u8; PUBLIC_KEY_LENGTH]);

impl PublicKey {
    /// Construct a public key from exactly 32 bytes.
    pub const fn from_bytes(bytes: [u8; PUBLIC_KEY_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Construct a public key from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, FixedBytesError> {
        Ok(Self(copy_fixed(bytes)?))
    }

    /// Construct a public key from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, HexDecodeError> {
        Ok(Self(hex_fixed(hex)?))
    }

    /// Return the raw 32 public-key bytes.
    pub const fn as_bytes(&self) -> &[u8; PUBLIC_KEY_LENGTH] {
        &self.0
    }

    /// Return the raw 32 public-key bytes by value.
    pub const fn to_bytes(self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.0
    }

    /// Return the lowercase hex representation of this key.
    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }

    /// Validate that this byte string is a usable Ed25519 public key.
    pub fn validate(&self) -> Result<(), VerifyError> {
        self.verifying_key().map(|_| ())
    }

    pub(crate) fn verifying_key(&self) -> Result<VerifyingKey, VerifyError> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(VerifyError::InvalidPublicKey)?;
        if key.is_weak() {
            return Err(VerifyError::WeakPublicKey);
        }
        Ok(key)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PublicKey").field(&self.to_hex()).finish()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_fixed_bytes(deserializer, "ed25519 public key").map(Self)
    }
}

/// A Carrel Ed25519 seed secret.
pub struct SecretKey(Zeroizing<[u8; SECRET_KEY_LENGTH]>);

impl SecretKey {
    /// Construct a secret key from a 32-byte Ed25519 seed.
    pub fn from_seed(seed: [u8; SECRET_KEY_LENGTH]) -> Self {
        Self(Zeroizing::new(seed))
    }

    /// Return the raw seed bytes.
    ///
    /// This is exposed for key persistence. Callers should avoid keeping the
    /// returned reference alive longer than necessary.
    pub fn as_bytes(&self) -> &[u8; SECRET_KEY_LENGTH] {
        &self.0
    }

    /// Return a copy of the raw seed bytes.
    ///
    /// This is intended for encrypted persistence and tests.
    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        *self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// An Ed25519 keypair used for signing Carrel facts.
pub struct Keypair {
    public: PublicKey,
    secret: SecretKey,
}

impl Keypair {
    /// Generate a new keypair from operating-system randomness.
    pub fn generate() -> Self {
        let mut seed = Zeroizing::new([0_u8; SECRET_KEY_LENGTH]);
        OsRng.fill_bytes(seed.as_mut());
        Self::from_seed(&seed)
    }

    /// Deterministically derive a keypair from a 32-byte Ed25519 seed.
    pub fn from_seed(seed: &[u8; SECRET_KEY_LENGTH]) -> Self {
        let signing = SigningKey::from_bytes(seed);
        Self {
            public: PublicKey(signing.verifying_key().to_bytes()),
            secret: SecretKey::from_seed(*seed),
        }
    }

    /// Return this keypair's public key.
    pub const fn public(&self) -> &PublicKey {
        &self.public
    }

    /// Return a copy of the seed bytes for encrypted or local persistence.
    pub fn to_seed_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.secret.to_bytes()
    }

    /// Sign arbitrary bytes with this keypair.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        let signing = SigningKey::from_bytes(self.secret.as_bytes());
        Signature(signing.sign(msg).to_bytes())
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.public)
            .field("secret", &"SecretKey(..)")
            .finish()
    }
}

/// A Carrel Ed25519 signature.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Signature([u8; SIGNATURE_LENGTH]);

impl Signature {
    /// Construct a signature from exactly 64 bytes.
    pub const fn from_bytes(bytes: [u8; SIGNATURE_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Construct a signature from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, FixedBytesError> {
        Ok(Self(copy_fixed(bytes)?))
    }

    /// Construct a signature from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, HexDecodeError> {
        Ok(Self(hex_fixed(hex)?))
    }

    /// Return the raw 64 signature bytes.
    pub const fn as_bytes(&self) -> &[u8; SIGNATURE_LENGTH] {
        &self.0
    }

    /// Return the raw 64 signature bytes by value.
    pub const fn to_bytes(self) -> [u8; SIGNATURE_LENGTH] {
        self.0
    }

    /// Return the lowercase hex representation of this signature.
    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature").field(&self.to_hex()).finish()
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_fixed_bytes(deserializer, "ed25519 signature").map(Self)
    }
}

/// Verify a raw byte signature against a public key.
pub fn verify(pub_key: &PublicKey, msg: &[u8], sig: &Signature) -> Result<(), VerifyError> {
    let verifying = pub_key.verifying_key()?;
    let dalek_sig = DalekSignature::from_bytes(sig.as_bytes());
    verifying
        .verify_strict(msg, &dalek_sig)
        .map_err(VerifyError::SignatureRejected)
}

/// Errors returned while verifying signatures.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The public key bytes do not decode to a usable Ed25519 point.
    #[error("invalid ed25519 public key")]
    InvalidPublicKey(#[source] ed25519_dalek::SignatureError),

    /// The public key is a weak low-order Ed25519 point.
    #[error("weak ed25519 public key")]
    WeakPublicKey,

    /// The signature does not verify for the given public key and message.
    #[error("ed25519 signature verification failed")]
    SignatureRejected(#[source] ed25519_dalek::SignatureError),
}

/// Error returned when a byte slice is not the expected fixed length.
#[derive(Debug, thiserror::Error)]
#[error("expected {expected} bytes, got {actual}")]
pub struct FixedBytesError {
    /// The required number of bytes.
    pub expected: usize,
    /// The number of bytes supplied by the caller.
    pub actual: usize,
}

/// Errors returned while decoding hex-encoded keys or signatures.
#[derive(Debug, thiserror::Error)]
pub enum HexDecodeError {
    /// The value was not valid lowercase or uppercase hex.
    #[error("invalid hex")]
    InvalidHex(#[from] hex::FromHexError),

    /// The decoded value had the wrong length.
    #[error(transparent)]
    InvalidLength(#[from] FixedBytesError),
}

fn copy_fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], FixedBytesError> {
    bytes.try_into().map_err(|_| FixedBytesError {
        expected: N,
        actual: bytes.len(),
    })
}

fn hex_fixed<const N: usize>(value: &str) -> Result<[u8; N], HexDecodeError> {
    let bytes = hex::decode(value)?;
    Ok(copy_fixed(&bytes)?)
}

fn deserialize_fixed_bytes<'de, D, const N: usize>(
    deserializer: D,
    name: &'static str,
) -> Result<[u8; N], D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_bytes(FixedBytesVisitor::<N> { name })
}

struct FixedBytesVisitor<const N: usize> {
    name: &'static str,
}

impl<'de, const N: usize> Visitor<'de> for FixedBytesVisitor<N> {
    type Value = [u8; N];

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} as {N} bytes", self.name)
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        copy_fixed(value).map_err(E::custom)
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(&value)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut bytes = [0_u8; N];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = seq
                .next_element()?
                .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
        }

        if seq.next_element::<u8>()?.is_some() {
            return Err(serde::de::Error::invalid_length(N + 1, &self));
        }

        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Keypair, SecretKey, Signature, verify};

    #[test]
    fn generated_keypair_signs_and_verifies() {
        let keypair = Keypair::generate();
        let signature = keypair.sign(b"quiet reading");

        verify(keypair.public(), b"quiet reading", &signature).unwrap();
    }

    #[test]
    fn verification_fails_for_wrong_key_or_message() {
        let signer = Keypair::from_seed(&[1_u8; 32]);
        let other = Keypair::from_seed(&[2_u8; 32]);
        let signature = signer.sign(b"message");

        verify(other.public(), b"message", &signature).unwrap_err();
        verify(signer.public(), b"tampered", &signature).unwrap_err();
    }

    #[test]
    fn verification_fails_for_tampered_signature() {
        let signer = Keypair::from_seed(&[3_u8; 32]);
        let mut signature = signer.sign(b"message").to_bytes();
        signature[0] ^= 1;
        let signature = Signature::from_bytes(signature);

        verify(signer.public(), b"message", &signature).unwrap_err();
    }

    #[test]
    fn secret_key_uses_drop_zeroization() {
        assert!(std::mem::needs_drop::<SecretKey>());
    }

    proptest! {
        #[test]
        fn sign_then_verify_succeeds(seed in any::<[u8; 32]>(), message in prop::collection::vec(any::<u8>(), 0..1024)) {
            let keypair = Keypair::from_seed(&seed);
            let signature = keypair.sign(&message);

            verify(keypair.public(), &message, &signature)?;
        }

        #[test]
        fn wrong_public_key_rejects_signature(
            seed_a in any::<[u8; 32]>(),
            seed_b in any::<[u8; 32]>(),
            message in prop::collection::vec(any::<u8>(), 0..1024),
        ) {
            prop_assume!(seed_a != seed_b);

            let signer = Keypair::from_seed(&seed_a);
            let other = Keypair::from_seed(&seed_b);
            let signature = signer.sign(&message);

            prop_assert!(verify(other.public(), &message, &signature).is_err());
        }
    }
}
