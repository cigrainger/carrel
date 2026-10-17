//! Verified certificates and the installation's explicitly chosen self identity.

use std::collections::{BTreeMap, BTreeSet};

use carrel_core::identity::{DeviceCert, Keypair, PublicKey, RevocationCert};
use cozo::{DataValue, Validity};

use crate::private_library::{QUEUE_PRIVATE_FACT, private_fact_params};
use crate::{Result, Store, StoreError};
use carrel_core::private_library::PrivateFact;

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

    pub(crate) fn identity_changed(&self) {
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
        self.check_recovery_binding(&cert.master_pubkey, device.public())?;
        self.require_recovery_author_certificates(&cert.master_pubkey, &BTreeSet::new())?;
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
        let mut params = certificate_params(cert)?;
        params.extend(private_fact_params(&PrivateFact::Certificate(
            cert.clone(),
        ))?);
        self.query_with_params(
            &format!(
                "{}\n{QUEUE_PRIVATE_FACT}",
                r#"
            { ?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]]
              :put device_certificate {master_pubkey, device_pubkey => device_cert} }
            { ?[singleton, master_pubkey, device_pubkey] <- [[0, $master, $device]]
              :put self_identity {singleton => master_pubkey, device_pubkey} }
        "#
            ),
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
        self.accept_self_device_certificate_unlocked(cert)
    }

    pub(crate) fn accept_self_certificate_from(
        &self,
        cert: &DeviceCert,
        sender: &PublicKey,
    ) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        self.require_private_sender(sender, None)?;
        self.accept_self_device_certificate_unlocked(cert)
    }

    fn accept_self_device_certificate_unlocked(&self, cert: &DeviceCert) -> Result<()> {
        cert.verify()?;
        self.require_bound_master(&cert.master_pubkey)?;
        if cert.device_pubkey == cert.master_pubkey {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.require_matching_certificate(cert)?;
        let mut params = certificate_params(cert)?;
        params.extend(private_fact_params(&PrivateFact::Certificate(
            cert.clone(),
        ))?);
        self.query_with_params(
            &format!("{{ ?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]] :put device_certificate {{master_pubkey, device_pubkey => device_cert}} }}\n{QUEUE_PRIVATE_FACT}"),
            params,
        )?;
        self.identity_changed();
        Ok(())
    }

    /// Original revocation proofs, including devices whose certificate is absent.
    pub fn self_revocation_proofs(&self) -> Result<Vec<RevocationCert>> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        let rows = self.query_with_params(
            "?[proof] := *device_revocation{master_pubkey: $master, revocation_cert: proof}",
            BTreeMap::from([(
                "master".into(),
                DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
            )]),
        )?;
        rows.rows
            .iter()
            .map(|row| {
                let proof = RevocationCert::from_bytes(bytes(&row[0])?)?;
                if proof.master_pubkey != binding.master_pubkey {
                    return Err(StoreError::IdentityMasterMismatch);
                }
                Ok(proof)
            })
            .collect()
    }

    /// Persist a master's signed revocation even if its certificate is not here yet.
    pub fn accept_self_revocation(&self, cert: &RevocationCert) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        self.accept_self_revocation_unlocked(cert)
    }

    pub(crate) fn accept_self_revocation_from(
        &self,
        cert: &RevocationCert,
        sender: &PublicKey,
    ) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("identity write lock");
        self.require_private_sender(sender, None)?;
        self.accept_self_revocation_unlocked(cert)
    }

    fn accept_self_revocation_unlocked(&self, cert: &RevocationCert) -> Result<()> {
        cert.verify()?;
        self.require_bound_master(&cert.master_pubkey)?;
        let bytes = cert.to_bytes().map_err(|error| StoreError::Signing {
            context: "device revocation",
            message: error.to_string(),
        })?;
        let mut params = private_fact_params(&PrivateFact::Revocation(cert.clone()))?;
        params.extend([
            ("master".into(), key_value(&cert.master_pubkey)),
            ("device".into(), key_value(&cert.device_pubkey)),
            (
                "at".into(),
                DataValue::Validity(Validity::from((cert.revoked_at_micros, true))),
            ),
            ("cert".into(), DataValue::Bytes(bytes)),
        ]);
        self.query_with_params(
            &format!("{{ ?[master_pubkey, device_pubkey, revoked_at, revocation_cert] <- [[$master, $device, $at, $cert]] :put device_revocation {{master_pubkey, device_pubkey, revoked_at => revocation_cert}} }}\n{QUEUE_PRIVATE_FACT}"),
            params,
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

    /// Whether the bound master has a permanent revocation or local block for a key.
    /// Denial applies even when that key's certificate has not arrived yet.
    pub fn self_device_denied(&self, device: &PublicKey) -> Result<bool> {
        let _guard = self.identity_lock.lock().expect("identity read lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.device_denied(&binding.master_pubkey, device)
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
            if let Some(origin) = self.restore_origin()? {
                if origin.device_pubkey != *device {
                    return Ok(BTreeSet::new());
                }
                let mut permitted = BTreeSet::from([*device]);
                permitted.extend(origin.legacy_devices);
                return Ok(permitted);
            }
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

    /// Read an original certificate during explicit local setup, without binding a Root.
    pub fn device_certificate(
        &self,
        master: &PublicKey,
        device: &PublicKey,
    ) -> Result<Option<DeviceCert>> {
        let rows = self.query_with_params("?[cert] := *device_certificate{master_pubkey: $master, device_pubkey: $device, device_cert: cert}",
            BTreeMap::from([("master".into(), key_value(master)), ("device".into(), key_value(device))]))?;
        rows.rows
            .first()
            .map(|row| {
                let cert = DeviceCert::from_bytes(bytes(&row[0])?)?;
                if cert.master_pubkey != *master || cert.device_pubkey != *device {
                    return Err(StoreError::IdentityCertificateConflict);
                }
                Ok(cert)
            })
            .transpose()
    }

    pub(crate) fn require_matching_certificate(&self, cert: &DeviceCert) -> Result<()> {
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

    pub(crate) fn device_denied(&self, master: &PublicKey, device: &PublicKey) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{EditNoteInput, PutNoteInput};

    #[test]
    fn recovery_pairing_denial_is_visible_before_the_device_certificate_arrives() {
        for revoked in [false, true] {
            let store = Store::open_in_memory().unwrap();
            store.migrate().unwrap();
            let master = Keypair::generate();
            let device = Keypair::generate();
            let denied = Keypair::generate();
            assert!(matches!(
                store.self_device_denied(denied.public()),
                Err(StoreError::IdentityUnbound)
            ));
            store
                .bind_self_identity(&DeviceCert::new(&master, device.public(), None), &device)
                .unwrap();
            assert!(!store.self_device_denied(denied.public()).unwrap());
            if revoked {
                store
                    .accept_self_revocation(&RevocationCert::new_at_micros(
                        &master,
                        denied.public(),
                        i64::MAX - 1,
                    ))
                    .unwrap();
            } else {
                store.query_with_params(
                    "?[master_pubkey,device_pubkey,reason] <- [[$master,$device,'local denial']] :put device_local_block {master_pubkey,device_pubkey=>reason}",
                    BTreeMap::from([
                        ("master".into(), key_value(master.public())),
                        ("device".into(), key_value(denied.public())),
                    ]),
                ).unwrap();
            }
            assert!(store.self_device_denied(denied.public()).unwrap());
            assert!(
                store
                    .device_certificate(master.public(), denied.public())
                    .unwrap()
                    .is_none()
            );
            assert!(
                !store
                    .self_devices()
                    .unwrap()
                    .iter()
                    .any(|known| known.certificate.device_pubkey == *denied.public())
            );
            // Certificate delivery retains history, without granting operational access.
            store
                .accept_self_device_certificate(&DeviceCert::new(&master, denied.public(), None))
                .unwrap();
            assert!(store.self_device_denied(denied.public()).unwrap());
            assert!(!store.self_device_authorized(denied.public()).unwrap());
            assert!(!store.self_device_denied(device.public()).unwrap());
            assert!(store.self_device_authorized(device.public()).unwrap());
        }
    }

    #[test]
    fn recovery_direct_binding_requires_verified_retained_author_certificates() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let master = Keypair::generate();
        let original = Keypair::generate();
        let intermediate = Keypair::generate();
        let fresh = Keypair::generate();
        let original_certificate = DeviceCert::new_at_micros(
            &master,
            original.public(),
            10,
            Some("Original desktop".into()),
        );
        store
            .bind_self_identity(&original_certificate, &original)
            .unwrap();
        store
            .prepare_restored_archive(Some(*original.public()), &intermediate)
            .unwrap();
        let note = store
            .put_note(
                PutNoteInput {
                    body: "Locally authored between restorations".into(),
                    target_kind: None,
                    target_id: None,
                    created_at_micros: Some(1),
                },
                &intermediate,
            )
            .unwrap();
        store
            .prepare_restored_archive(Some(*intermediate.public()), &fresh)
            .unwrap();
        let local_certificate = DeviceCert::new(&master, fresh.public(), None);
        // A valid certificate under another root cannot supply author coverage.
        let foreign = DeviceCert::new(&Keypair::generate(), intermediate.public(), None);
        store.query_with_params(
            "?[master_pubkey,device_pubkey,device_cert] <- [[$master,$device,$cert]] :put device_certificate {master_pubkey,device_pubkey=>device_cert}",
            certificate_params(&foreign).unwrap(),
        ).unwrap();
        let before = store
            .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
            .unwrap()
            .rows;
        assert!(matches!(
            store.bind_self_identity(&local_certificate, &fresh),
            Err(StoreError::InvalidInput { context: "archive recovery", message })
                if message.contains("master authorization is required")
        ));
        assert!(store.self_identity().unwrap().is_none());
        assert!(
            store
                .device_certificate(master.public(), fresh.public())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
                .unwrap()
                .rows,
            before
        );
        let local_edit = store
            .edit_note(
                EditNoteInput {
                    id: note.id,
                    body: "Still editable while unbound".into(),
                    expected_revision_id: note.revision_id,
                    now_micros: Some(2),
                },
                &fresh,
            )
            .unwrap();
        let author = DeviceCert::new_at_micros(
            &master,
            intermediate.public(),
            15,
            Some("Intermediate desktop".into()),
        );
        store.query_with_params(
            "?[master_pubkey,device_pubkey,device_cert] <- [[$master,$device,$cert]] :put device_certificate {master_pubkey,device_pubkey=>device_cert}",
            certificate_params(&author).unwrap(),
        ).unwrap();
        store
            .bind_self_identity(&local_certificate, &fresh)
            .unwrap();
        assert_eq!(
            store
                .device_certificate(master.public(), original.public())
                .unwrap(),
            Some(original_certificate)
        );
        assert_eq!(
            store
                .device_certificate(master.public(), intermediate.public())
                .unwrap(),
            Some(author)
        );
        store
            .edit_note(
                EditNoteInput {
                    id: local_edit.id,
                    body: "Still editable after binding".into(),
                    expected_revision_id: local_edit.revision_id,
                    now_micros: Some(3),
                },
                &fresh,
            )
            .unwrap()
            .verify_signature()
            .unwrap();
    }

    #[test]
    fn recovery_unbound_archive_cannot_bind_without_authorizing_its_original_key() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let original = Keypair::generate();
        let fresh = Keypair::generate();
        let master = Keypair::generate();
        store
            .prepare_restored_archive(Some(*original.public()), &fresh)
            .unwrap();
        assert!(
            store
                .bind_self_identity(&DeviceCert::new(&master, fresh.public(), None), &fresh)
                .is_err()
        );
        assert!(store.self_identity().unwrap().is_none());
        assert!(
            store
                .annotation_creator_permissions(fresh.public())
                .unwrap()
                .contains(original.public())
        );
        store
            .authorize_restored_identity(&master, &fresh, "Recovered")
            .unwrap();
        assert!(
            store
                .device_certificate(master.public(), original.public())
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .annotation_creator_permissions(fresh.public())
                .unwrap()
                .contains(original.public())
        );
    }
}
