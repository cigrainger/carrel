//! Master-to-device authorization certificates.

use std::io::Cursor;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::keypair::{Keypair, PublicKey, Signature, VerifyError};
use super::signing::{
    CanonicalVerifyError, SerializeError, canonical_bytes, sign_canonical, verify_canonical,
};

/// A certificate proving that a master identity authorized a device key.
///
/// The master signature covers `master_pubkey`, `device_pubkey`,
/// `authorized_at_micros`, and `device_name` as canonical CBOR. It does not
/// cover the `master_signature` field itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceCert {
    /// The user's stable master identity.
    pub master_pubkey: PublicKey,
    /// The device sub-key authorized by the master identity.
    pub device_pubkey: PublicKey,
    /// The authorization timestamp in microseconds since the Unix epoch.
    pub authorized_at_micros: i64,
    /// A local, user-chosen label for the device.
    pub device_name: Option<String>,
    /// The master key's signature over the certificate payload.
    pub master_signature: Signature,
}

impl DeviceCert {
    /// Create a certificate for `device_pubkey` using the current time.
    pub fn new(master: &Keypair, device_pubkey: &PublicKey, device_name: Option<String>) -> Self {
        Self::new_at_micros(master, device_pubkey, now_micros(), device_name)
    }

    /// Create a certificate for `device_pubkey` at a supplied timestamp.
    pub fn new_at_micros(
        master: &Keypair,
        device_pubkey: &PublicKey,
        authorized_at_micros: i64,
        device_name: Option<String>,
    ) -> Self {
        let payload = DeviceCertPayload {
            master_pubkey: master.public(),
            device_pubkey,
            authorized_at_micros,
            device_name: &device_name,
        };
        let master_signature =
            sign_canonical(master, &payload).expect("device cert payload serializes");

        Self {
            master_pubkey: *master.public(),
            device_pubkey: *device_pubkey,
            authorized_at_micros,
            device_name,
            master_signature,
        }
    }

    /// Verify the certificate's master signature and key shapes.
    pub fn verify(&self) -> Result<(), CertError> {
        self.master_pubkey
            .validate()
            .map_err(CertError::InvalidMasterPublicKey)?;
        self.device_pubkey
            .validate()
            .map_err(CertError::InvalidDevicePublicKey)?;
        verify_canonical(&self.master_pubkey, &self.payload(), &self.master_signature)
            .map_err(CertError::InvalidDeviceCertSignature)
    }

    /// Serialize this certificate to canonical CBOR bytes for Cozo storage.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        canonical_bytes(self)
    }

    /// Deserialize and verify a certificate from canonical CBOR bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CertError> {
        let cert: Self = ciborium::de::from_reader(Cursor::new(bytes))
            .map_err(|source| CertError::Deserialize(source.to_string()))?;
        cert.verify()?;
        Ok(cert)
    }

    fn payload(&self) -> DeviceCertPayload<'_> {
        DeviceCertPayload {
            master_pubkey: &self.master_pubkey,
            device_pubkey: &self.device_pubkey,
            authorized_at_micros: self.authorized_at_micros,
            device_name: &self.device_name,
        }
    }
}

/// A certificate proving that a master identity revoked a device key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevocationCert {
    /// The user's stable master identity.
    pub master_pubkey: PublicKey,
    /// The device sub-key revoked by the master identity.
    pub device_pubkey: PublicKey,
    /// The revocation timestamp in microseconds since the Unix epoch.
    pub revoked_at_micros: i64,
    /// The master key's signature over the revocation payload.
    pub master_signature: Signature,
}

impl RevocationCert {
    /// Create a revocation certificate using the current time.
    pub fn new(master: &Keypair, device_pubkey: &PublicKey) -> Self {
        Self::new_at_micros(master, device_pubkey, now_micros())
    }

    /// Create a revocation certificate at a supplied timestamp.
    pub fn new_at_micros(
        master: &Keypair,
        device_pubkey: &PublicKey,
        revoked_at_micros: i64,
    ) -> Self {
        let payload = RevocationCertPayload {
            master_pubkey: master.public(),
            device_pubkey,
            revoked_at_micros,
        };
        let master_signature =
            sign_canonical(master, &payload).expect("revocation cert payload serializes");

        Self {
            master_pubkey: *master.public(),
            device_pubkey: *device_pubkey,
            revoked_at_micros,
            master_signature,
        }
    }

    /// Verify the revocation certificate's master signature and key shapes.
    pub fn verify(&self) -> Result<(), CertError> {
        self.master_pubkey
            .validate()
            .map_err(CertError::InvalidMasterPublicKey)?;
        self.device_pubkey
            .validate()
            .map_err(CertError::InvalidDevicePublicKey)?;
        verify_canonical(&self.master_pubkey, &self.payload(), &self.master_signature)
            .map_err(CertError::InvalidRevocationSignature)
    }

    /// Serialize this revocation certificate to canonical CBOR bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        canonical_bytes(self)
    }

    /// Deserialize and verify a revocation certificate from canonical CBOR bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CertError> {
        let cert: Self = ciborium::de::from_reader(Cursor::new(bytes))
            .map_err(|source| CertError::Deserialize(source.to_string()))?;
        cert.verify()?;
        Ok(cert)
    }

    fn payload(&self) -> RevocationCertPayload<'_> {
        RevocationCertPayload {
            master_pubkey: &self.master_pubkey,
            device_pubkey: &self.device_pubkey,
            revoked_at_micros: self.revoked_at_micros,
        }
    }
}

/// A verified device authorization plus optional revocation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceAuthorization {
    /// The authorization certificate issued by the master key.
    pub cert: DeviceCert,
    /// A revocation certificate for the same device, if present.
    pub revocation: Option<RevocationCert>,
}

impl DeviceAuthorization {
    /// Verify that the device is currently authorized.
    pub fn verify(&self) -> Result<(), CertError> {
        self.cert.verify()?;

        let Some(revocation) = &self.revocation else {
            return Ok(());
        };

        revocation.verify()?;

        if revocation.master_pubkey != self.cert.master_pubkey
            || revocation.device_pubkey != self.cert.device_pubkey
        {
            return Err(CertError::RevocationMismatch);
        }

        Err(CertError::DeviceRevoked {
            revoked_at_micros: revocation.revoked_at_micros,
        })
    }
}

#[derive(Serialize)]
struct DeviceCertPayload<'a> {
    master_pubkey: &'a PublicKey,
    device_pubkey: &'a PublicKey,
    authorized_at_micros: i64,
    device_name: &'a Option<String>,
}

#[derive(Serialize)]
struct RevocationCertPayload<'a> {
    master_pubkey: &'a PublicKey,
    device_pubkey: &'a PublicKey,
    revoked_at_micros: i64,
}

/// Errors returned while verifying identity certificates.
#[derive(Debug, thiserror::Error)]
pub enum CertError {
    /// The master public key bytes are not a usable Ed25519 key.
    #[error("invalid master public key")]
    InvalidMasterPublicKey(#[source] VerifyError),

    /// The device public key bytes are not a usable Ed25519 key.
    #[error("invalid device public key")]
    InvalidDevicePublicKey(#[source] VerifyError),

    /// The device authorization signature does not verify.
    #[error("device authorization signature did not verify")]
    InvalidDeviceCertSignature(#[source] CanonicalVerifyError),

    /// The revocation signature does not verify.
    #[error("device revocation signature did not verify")]
    InvalidRevocationSignature(#[source] CanonicalVerifyError),

    /// The revocation certificate names a different master or device key.
    #[error("device revocation does not match authorization")]
    RevocationMismatch,

    /// The device has been revoked.
    #[error("device was revoked at {revoked_at_micros}")]
    DeviceRevoked {
        /// Revocation timestamp in microseconds since the Unix epoch.
        revoked_at_micros: i64,
    },

    /// The certificate bytes could not be decoded as CBOR.
    #[error("failed to deserialize certificate: {0}")]
    Deserialize(String),
}

fn now_micros() -> i64 {
    let micros = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000;
    i64::try_from(micros).expect("current timestamp fits in i64 microseconds")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{DeviceAuthorization, DeviceCert, RevocationCert};
    use crate::identity::Keypair;

    #[test]
    fn device_cert_round_trips_and_verifies() {
        let master = Keypair::from_seed(&[1_u8; 32]);
        let device = Keypair::from_seed(&[2_u8; 32]);
        let cert =
            DeviceCert::new_at_micros(&master, device.public(), 1_700_000, Some("laptop".into()));

        cert.verify().unwrap();

        let bytes = cert.to_bytes().unwrap();
        let parsed = DeviceCert::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, cert);
    }

    #[test]
    fn tampered_device_cert_fails_verification() {
        let master = Keypair::from_seed(&[3_u8; 32]);
        let device = Keypair::from_seed(&[4_u8; 32]);
        let mut cert = DeviceCert::new_at_micros(&master, device.public(), 1, None);

        cert.device_name = Some("tampered".into());

        cert.verify().unwrap_err();
    }

    #[test]
    fn revocation_refuses_authorization() {
        let master = Keypair::from_seed(&[5_u8; 32]);
        let device = Keypair::from_seed(&[6_u8; 32]);
        let cert = DeviceCert::new_at_micros(&master, device.public(), 1, Some("phone".into()));
        let revocation = RevocationCert::new_at_micros(&master, device.public(), 2);
        let authorization = DeviceAuthorization {
            cert,
            revocation: Some(revocation),
        };

        authorization.verify().unwrap_err();
    }

    #[test]
    fn mismatched_revocation_is_rejected() {
        let master = Keypair::from_seed(&[7_u8; 32]);
        let device = Keypair::from_seed(&[8_u8; 32]);
        let other_device = Keypair::from_seed(&[9_u8; 32]);
        let cert = DeviceCert::new_at_micros(&master, device.public(), 1, None);
        let revocation = RevocationCert::new_at_micros(&master, other_device.public(), 2);
        let authorization = DeviceAuthorization {
            cert,
            revocation: Some(revocation),
        };

        authorization.verify().unwrap_err();
    }

    proptest! {
        #[test]
        fn generated_certs_verify(
            master_seed in any::<[u8; 32]>(),
            device_seed in any::<[u8; 32]>(),
            authorized_at_micros in any::<i64>(),
            name in prop::option::of("\\PC{0,64}"),
        ) {
            let master = Keypair::from_seed(&master_seed);
            let device = Keypair::from_seed(&device_seed);
            let cert = DeviceCert::new_at_micros(&master, device.public(), authorized_at_micros, name);

            prop_assert!(cert.verify().is_ok());
        }

        #[test]
        fn tampered_generated_certs_fail(
            master_seed in any::<[u8; 32]>(),
            device_seed in any::<[u8; 32]>(),
            authorized_at_micros in any::<i64>(),
        ) {
            let master = Keypair::from_seed(&master_seed);
            let device = Keypair::from_seed(&device_seed);
            let mut cert = DeviceCert::new_at_micros(&master, device.public(), authorized_at_micros, None);
            cert.authorized_at_micros = cert.authorized_at_micros.saturating_add(1);

            prop_assert!(cert.verify().is_err());
        }
    }
}
