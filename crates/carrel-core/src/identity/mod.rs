//! Cryptographic identity primitives for Carrel.
//!
//! Identity is represented as an Ed25519 master key plus device sub-keys. This
//! module keeps the pure signing and certificate rules in `carrel-core`; disk
//! persistence lives in `carrel-store`.

mod cert;
mod keypair;
mod signing;

pub use cert::{CertError, DeviceAuthorization, DeviceCert, RevocationCert};
pub use keypair::{
    FixedBytesError, HexDecodeError, Keypair, PublicKey, SECRET_KEY_LENGTH, SecretKey, Signature,
    VerifyError, verify,
};
pub use signing::{
    CanonicalVerifyError, SerializeError, canonical_bytes, sign_canonical, verify_canonical,
};
