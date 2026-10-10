//! Verified certificates and the installation's explicitly chosen self identity.

use std::collections::{BTreeMap, BTreeSet};

use carrel_core::identity::{DeviceCert, Keypair, PublicKey, RevocationCert};
use cozo::{DataValue, Validity};

use crate::{Result, Store, StoreError};

/// The stable master and independent operational key bound to this installation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelfIdentity {
    /// The chosen master public key; incoming data cannot replace it.
    pub master_pubkey: PublicKey,
    /// This installation's existing device key.
    pub device_pubkey: PublicKey,
}

/// A verified certificate with permanent revocation or legacy blocking state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelfDevice {
    /// Immutable master-signed authorization.
    pub certificate: DeviceCert,
    /// The latest known signed proof; any proof denies authorization.
    pub revocation: Option<RevocationCert>,
    /// An old unsigned revocation cannot be promoted to a network proof.
    pub locally_blocked: bool,
}

impl SelfDevice {
    /// Whether this verified device remains authorized for private sync.
    pub fn is_authorized(&self) -> bool {
        self.revocation.is_none() && !self.locally_blocked
    }
}

impl Store {
    /// Subscribe to successful typed identity writes without exposing any facts.
    ///
    /// Consumers re-query Cozo; coalesced wake-ups are sufficient. Raw queries
    /// are a development escape hatch and do not emit authorization events.
    pub fn subscribe_identity_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.identity_revision.subscribe()
    }

    fn identity_changed(&self) {
        self.identity_revision.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
    }

    /// Return the explicitly bound identity, without inferring trust from peers.
    pub fn self_identity(&self) -> Result<Option<SelfIdentity>> {
        let rows = self.query("?[master, device] := *self_identity{singleton: 0, master_pubkey: master, device_pubkey: device}")?;
        rows.rows
            .first()
            .map(|row| {
                Ok(SelfIdentity {
                    master_pubkey: public_key(&row[0])?,
                    device_pubkey: public_key(&row[1])?,
                })
            })
            .transpose()
    }

    /// Bind the chosen master to the local key, preserving existing annotations.
    ///
    /// This is a local setup operation, never an incoming fact handler. Possession
    /// of the operational key is required; private master material is unnecessary.
    pub fn bind_self_identity(&self, cert: &DeviceCert, device: &Keypair) -> Result<SelfIdentity> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        cert.verify()?;
        let binding = SelfIdentity {
            master_pubkey: cert.master_pubkey,
            device_pubkey: *device.public(),
        };
        if cert.device_pubkey != binding.device_pubkey || cert.master_pubkey == cert.device_pubkey {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        if self
            .self_identity()?
            .is_some_and(|existing| existing != binding)
        {
            return Err(StoreError::IdentityBindingConflict);
        }
        self.require_matching_certificate(cert)?;
        if self.device_denied(&cert.master_pubkey, &cert.device_pubkey)? {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let params = certificate_params(cert)?;
        self.query_with_params(
            r#"
            { ?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]]
              :put device_certificate {master_pubkey, device_pubkey => device_cert} }
            { ?[singleton, master_pubkey, device_pubkey] <- [[0, $master, $device]]
              :put self_identity {singleton => master_pubkey, device_pubkey} }
        "#,
            params,
        )?;
        self.identity_changed();
        Ok(binding)
    }

    /// Accept a verified device certificate only under the locally bound master.
    ///
    /// A certificate arriving after its revocation remains useful as historical
    /// proof of authorship, but cannot reauthorize the device.
    pub fn accept_self_device_certificate(&self, cert: &DeviceCert) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        cert.verify()?;
        self.require_bound_master(&cert.master_pubkey)?;
        if cert.device_pubkey == cert.master_pubkey {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.require_matching_certificate(cert)?;
        self.query_with_params(
            "?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]] :put device_certificate {master_pubkey, device_pubkey => device_cert}",
            certificate_params(cert)?,
        )?;
        self.identity_changed();
        Ok(())
    }

    /// Persist a master's signed revocation even if its certificate is not here yet.
    pub fn accept_self_revocation(&self, cert: &RevocationCert) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        cert.verify()?;
        self.require_bound_master(&cert.master_pubkey)?;
        let bytes = cert.to_bytes().map_err(|error| StoreError::Signing {
            context: "device revocation",
            message: error.to_string(),
        })?;
        self.query_with_params(
            "?[master_pubkey, device_pubkey, revoked_at, revocation_cert] <- [[$master, $device, $at, $cert]] :put device_revocation {master_pubkey, device_pubkey, revoked_at => revocation_cert}",
            BTreeMap::from([
                ("master".into(), key_value(&cert.master_pubkey)),
                ("device".into(), key_value(&cert.device_pubkey)),
                ("at".into(), DataValue::Validity(Validity::from((cert.revoked_at_micros, true)))),
                ("cert".into(), DataValue::Bytes(bytes)),
            ]),
        )?;
        self.identity_changed();
        Ok(())
    }

    /// Read all self certificates and current proofs in one database snapshot.
    pub fn self_devices(&self) -> Result<Vec<SelfDevice>> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        // Cozo 0.7.6 cannot anti-join a stored validity read directly. Materialize
        // the current proofs in a derived rule before the missing-proof branch.
        let rows = self.query_with_params(r#"
            known_proof[device, at, proof] := *device_revocation{master_pubkey: $master, device_pubkey: device, revoked_at: at, revocation_cert: proof @ "END"}
            revocation[device, at, proof] := known_proof[device, at, proof]
            revocation[device, at, proof] := *device_certificate{master_pubkey: $master, device_pubkey: device}, not known_proof[device, _, _], at = null, proof = null
            blocked[device, block] := *device_local_block{master_pubkey: $master, device_pubkey: device}, block = true
            blocked[device, block] := *device_certificate{master_pubkey: $master, device_pubkey: device}, not *device_local_block{master_pubkey: $master, device_pubkey: device}, block = false
            ?[device, cert, at, proof, block] := *device_certificate{master_pubkey: $master, device_pubkey: device, device_cert: cert}, revocation[device, at, proof], blocked[device, block]
            :sort device
        "#, BTreeMap::from([("master".into(), key_value(&binding.master_pubkey))]))?;
        rows.rows
            .iter()
            .map(|row| {
                let device = public_key(&row[0])?;
                let certificate = DeviceCert::from_bytes(bytes(&row[1])?)?;
                if certificate.master_pubkey != binding.master_pubkey
                    || certificate.device_pubkey != device
                {
                    return Err(StoreError::IdentityMasterMismatch);
                }
                let revocation = if row[3] == DataValue::Null {
                    None
                } else {
                    let proof = RevocationCert::from_bytes(bytes(&row[3])?)?;
                    if proof.master_pubkey != binding.master_pubkey
                        || proof.device_pubkey != device
                        || row[2]
                            != DataValue::Validity(Validity::from((proof.revoked_at_micros, true)))
                    {
                        return Err(StoreError::IdentityMasterMismatch);
                    }
                    Some(proof)
                };
                let DataValue::Bool(locally_blocked) = row[4] else {
                    return Err(StoreError::UnexpectedValue {
                        context: "device_local_block",
                        value: "expected boolean".into(),
                    });
                };
                Ok(SelfDevice {
                    certificate,
                    revocation,
                    locally_blocked,
                })
            })
            .collect()
    }

    /// Whether the bound master currently authorizes a particular remote device.
    pub fn self_device_authorized(&self, device: &PublicKey) -> Result<bool> {
        let _guard = self.identity_lock.lock().expect("identity read lock");
        Ok(self
            .self_devices()?
            .iter()
            .any(|row| row.certificate.device_pubkey == *device && row.is_authorized()))
    }

    /// Creator keys whose annotations the installed device may alter.
    ///
    /// Before self-sync setup, ownership remains limited to the local key. A
    /// revoked creator's existing work remains editable by another valid device.
    pub fn annotation_creator_permissions(
        &self,
        device: &PublicKey,
    ) -> Result<BTreeSet<PublicKey>> {
        let _guard = self.identity_lock.lock().expect("identity read lock");
        self.annotation_creator_permissions_unlocked(device)
    }

    pub(crate) fn annotation_creator_permissions_unlocked(
        &self,
        device: &PublicKey,
    ) -> Result<BTreeSet<PublicKey>> {
        let Some(binding) = self.self_identity()? else {
            return Ok(BTreeSet::from([*device]));
        };
        if binding.device_pubkey != *device {
            return Ok(BTreeSet::new());
        }
        let devices = self.self_devices()?;
        if !devices
            .iter()
            .any(|row| row.certificate.device_pubkey == *device && row.is_authorized())
        {
            return Ok(BTreeSet::new());
        }
        Ok(devices
            .into_iter()
            .map(|row| row.certificate.device_pubkey)
            .collect())
    }

    fn require_bound_master(&self, master: &PublicKey) -> Result<()> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if binding.master_pubkey != *master {
            return Err(StoreError::IdentityMasterMismatch);
        }
        Ok(())
    }

    fn require_matching_certificate(&self, cert: &DeviceCert) -> Result<()> {
        let rows = self.query_with_params("?[cert] := *device_certificate{master_pubkey: $master, device_pubkey: $device, device_cert: cert}",
            BTreeMap::from([("master".into(), key_value(&cert.master_pubkey)), ("device".into(), key_value(&cert.device_pubkey))]))?;
        if let Some(row) = rows.rows.first() {
            let existing = DeviceCert::from_bytes(bytes(&row[0])?)?;
            if existing != *cert {
                return Err(StoreError::IdentityCertificateConflict);
            }
        }
        Ok(())
    }

    fn device_denied(&self, master: &PublicKey, device: &PublicKey) -> Result<bool> {
        let rows = self.query_with_params(r#"
            ?[device] := *device_revocation{master_pubkey: $master, device_pubkey: device @ "END"}, device = $device
            ?[device] := *device_local_block{master_pubkey: $master, device_pubkey: device}, device = $device
        "#, BTreeMap::from([("master".into(), key_value(master)), ("device".into(), key_value(device))]))?;
        Ok(!rows.rows.is_empty())
    }
}

pub(crate) fn certificate_params(cert: &DeviceCert) -> Result<BTreeMap<String, DataValue>> {
    let bytes = cert.to_bytes().map_err(|error| StoreError::Signing {
        context: "device certificate",
        message: error.to_string(),
    })?;
    Ok(BTreeMap::from([
        ("master".into(), key_value(&cert.master_pubkey)),
        ("device".into(), key_value(&cert.device_pubkey)),
        ("cert".into(), DataValue::Bytes(bytes)),
    ]))
}

fn key_value(key: &PublicKey) -> DataValue {
    DataValue::Bytes(key.as_bytes().to_vec())
}

pub(crate) fn public_key(value: &DataValue) -> Result<PublicKey> {
    PublicKey::from_slice(bytes(value)?).map_err(|_| StoreError::InvalidInput {
        context: "identity key",
        message: "expected 32 public-key bytes".into(),
    })
}

pub(crate) fn bytes(value: &DataValue) -> Result<&[u8]> {
    let DataValue::Bytes(bytes) = value else {
        return Err(StoreError::UnexpectedValue {
            context: "identity bytes",
            value: "expected bytes".into(),
        });
    };
    Ok(bytes)
}
