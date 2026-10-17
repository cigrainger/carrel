//! Local private document credentials and certified peer address hints.

use std::collections::{BTreeMap, BTreeSet};

use carrel_core::identity::{DeviceCert, Keypair, PublicKey, RevocationCert};
use carrel_core::private_library::PrivateFact;
use cozo::DataValue;

use crate::private_library::QUEUE_PRIVATE_FACT;
use crate::{Result, Store, StoreError, identity::bytes};

/// An explicitly chosen document, independent of identity key material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateDocument {
    /// Document namespace, checked against the capability by `carrel-sync`.
    pub namespace: [u8; 32],
    /// Bounded serialized write capability; confidential local configuration.
    pub capability: Vec<u8>,
}

/// A user-approved pairing grant, verified by the sync protocol before binding.
pub struct PairedIdentity {
    /// Certificate proving ownership of this installation's original key.
    pub certificate: DeviceCert,
    /// Explicit document credential from the verified grant.
    pub document: PrivateDocument,
    /// All historical operational certificates supplied by the Root.
    pub certificates: Vec<DeviceCert>,
    /// Permanent revocations, including ones preceding their certificate.
    pub revocations: Vec<RevocationCert>,
    /// Validated, bounded addresses of active certified peers.
    pub peers: Vec<(PublicKey, Vec<u8>)>,
}

impl Store {
    /// Commit identity, document, proofs and contacts in one transaction.
    /// A crash cannot leave a bound desktop which creates an unrelated document.
    /// This is an explicit local setup API, never an incoming fact handler.
    pub fn bind_paired_identity(&self, setup: &PairedIdentity, device: &Keypair) -> Result<()> {
        if setup.document.capability.is_empty()
            || setup.document.capability.len() > 4096
            || setup.certificates.len() > 512
            || setup.revocations.len() > 512
            || setup.peers.len() > 512
        {
            return Err(invalid("pairing grant bounds"));
        }
        let _guard = self.identity_lock.lock().expect("identity setup lock");
        let cert = &setup.certificate;
        cert.verify()?;
        self.check_recovery_binding(&cert.master_pubkey, device.public())?;
        if cert.device_pubkey != *device.public() || cert.master_pubkey == cert.device_pubkey {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let binding = crate::identity::SelfIdentity {
            master_pubkey: cert.master_pubkey,
            device_pubkey: cert.device_pubkey,
        };
        if self
            .self_identity()?
            .is_some_and(|existing| existing != binding)
        {
            return Err(StoreError::IdentityBindingConflict);
        }
        let mut certificates = BTreeMap::new();
        let mut facts = BTreeMap::new();
        for proof in std::iter::once(cert).chain(&setup.certificates) {
            proof.verify()?;
            if proof.master_pubkey != binding.master_pubkey {
                return Err(StoreError::IdentityMasterMismatch);
            }
            if proof.device_pubkey == binding.master_pubkey {
                return Err(StoreError::IdentityDeviceUnauthorized);
            }
            self.require_matching_certificate(proof)?;
            let encoded = proof.to_bytes().map_err(serialization)?;
            if certificates
                .insert(proof.device_pubkey, encoded.clone())
                .is_some_and(|old| old != encoded)
            {
                return Err(StoreError::IdentityCertificateConflict);
            }
            let fact = PrivateFact::Certificate(proof.clone());
            facts.insert(fact.key()?, fact.to_bytes()?);
        }
        self.require_recovery_author_certificates(
            &binding.master_pubkey,
            &certificates.keys().copied().collect::<BTreeSet<_>>(),
        )?;
        let mut removed = std::collections::BTreeSet::new();
        let mut revocations = Vec::new();
        for proof in &setup.revocations {
            proof.verify()?;
            if proof.master_pubkey != binding.master_pubkey {
                return Err(StoreError::IdentityMasterMismatch);
            }
            removed.insert(proof.device_pubkey);
            revocations.push(DataValue::List(vec![
                DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
                DataValue::Bytes(proof.device_pubkey.as_bytes().to_vec()),
                DataValue::Validity(cozo::Validity::from((proof.revoked_at_micros, true))),
                DataValue::Bytes(proof.to_bytes().map_err(serialization)?),
            ]));
            let fact = PrivateFact::Revocation(proof.clone());
            facts.insert(fact.key()?, fact.to_bytes()?);
        }
        if removed.contains(device.public())
            || self.device_denied(&binding.master_pubkey, device.public())?
        {
            // The unbound case permits one's own original key; the bound case
            // additionally enforces permanent local revocation and blocking.
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let mut peers = Vec::new();
        for (key, address) in &setup.peers {
            if address.is_empty()
                || address.len() > 4096
                || !certificates.contains_key(key)
                || removed.contains(key)
            {
                return Err(invalid("uncertified pairing peer"));
            }
            peers.push(DataValue::List(vec![
                DataValue::Bytes(key.as_bytes().to_vec()),
                DataValue::Bytes(address.clone()),
            ]));
        }
        let certificate_rows = certificates
            .into_iter()
            .map(|(key, encoded)| {
                DataValue::List(vec![
                    DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
                    DataValue::Bytes(key.as_bytes().to_vec()),
                    DataValue::Bytes(encoded),
                ])
            })
            .collect();
        let fact_rows = facts
            .into_iter()
            .map(|(key, encoded)| {
                DataValue::List(vec![DataValue::from(key), DataValue::Bytes(encoded)])
            })
            .collect();
        self.query_with_params(r#"
            { ?[master_pubkey, device_pubkey, device_cert] <- $certificates :put device_certificate {master_pubkey, device_pubkey => device_cert} }
            { ?[master_pubkey, device_pubkey, revoked_at, revocation_cert] <- $revocations :put device_revocation {master_pubkey, device_pubkey, revoked_at => revocation_cert} }
            { ?[key, encoded] <- $facts :put self_fact_history {key => encoded} }
            { ?[key, encoded] <- $facts :put self_fact_outbox {key => encoded} }
            { ?[singleton, master_pubkey, device_pubkey] <- [[0, $master, $device]] :put self_identity {singleton => master_pubkey, device_pubkey} }
            { ?[singleton, namespace, capability] <- [[0, $namespace, $capability]] :put self_document {singleton => namespace, capability} }
            { ?[device_pubkey, address] <- $peers :put self_peer_address {device_pubkey => address} }
        "#, BTreeMap::from([
            ("certificates".into(), DataValue::List(certificate_rows)), ("revocations".into(), DataValue::List(revocations)),
            ("facts".into(), DataValue::List(fact_rows)), ("peers".into(), DataValue::List(peers)),
            ("master".into(), DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec())), ("device".into(), DataValue::Bytes(binding.device_pubkey.as_bytes().to_vec())),
            ("namespace".into(), DataValue::Bytes(setup.document.namespace.to_vec())), ("capability".into(), DataValue::Bytes(setup.document.capability.clone())),
        ]))?;
        self.identity_changed();
        Ok(())
    }
    /// Read the durable document binding without inferring or creating one.
    pub fn private_document(&self) -> Result<Option<PrivateDocument>> {
        self.query(
            "?[namespace, capability] := *self_document{singleton: 0, namespace, capability}",
        )?
        .rows
        .first()
        .map(|row| {
            Ok(PrivateDocument {
                namespace: bytes(&row[0])?
                    .try_into()
                    .map_err(|_| invalid("namespace length"))?,
                capability: bytes(&row[1])?.to_vec(),
            })
        })
        .transpose()
    }

    /// Persist the first write capability; subsequent bindings must be identical.
    pub fn bind_private_document(&self, document: &PrivateDocument) -> Result<()> {
        if document.capability.is_empty() || document.capability.len() > 4096 {
            return Err(invalid("capability length"));
        }
        let _guard = self.identity_lock.lock().expect("identity write lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        self.query_with_params("?[singleton, namespace, capability] <- [[0, $namespace, $capability]] :put self_document {singleton => namespace, capability}", BTreeMap::from([
            ("namespace".into(), DataValue::Bytes(document.namespace.to_vec())),
            ("capability".into(), DataValue::Bytes(document.capability.clone())),
        ]))?;
        Ok(())
    }

    /// Explicit local review of a recovered desktop's certified connection code.
    /// The native layer also checks that the transport address names this key.
    pub fn register_recovered_desktop(
        &self,
        card: &carrel_core::sharing::ContactCard,
    ) -> Result<()> {
        card.verify()?;
        let _guard = self.identity_lock.lock().expect("recovered desktop lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        if card.certificate.master_pubkey != binding.master_pubkey {
            return Err(StoreError::IdentityMasterMismatch);
        }
        if card.certificate.device_pubkey == binding.device_pubkey
            || card.certificate.device_pubkey == binding.master_pubkey
            || self.device_denied(&binding.master_pubkey, &card.certificate.device_pubkey)?
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.require_matching_certificate(&card.certificate)?;
        let mut params = crate::identity::certificate_params(&card.certificate)?;
        params.extend(crate::private_library::private_fact_params(
            &PrivateFact::Certificate(card.certificate.clone()),
        )?);
        params.insert(
            "address".into(),
            DataValue::Bytes(card.address.as_bytes().to_vec()),
        );
        self.query_with_params(&format!(r#"
            {{ ?[master_pubkey,device_pubkey,device_cert] <- [[$master,$device,$cert]] :put device_certificate {{master_pubkey,device_pubkey=>device_cert}} }}
            {{ ?[device_pubkey,address] <- [[$device,$address]] :put self_peer_address {{device_pubkey=>address}} }}
            {QUEUE_PRIVATE_FACT}
        "#),params)?;
        self.identity_changed();
        Ok(())
    }

    /// Save an address only for an active device in the explicitly chosen Root.
    pub fn set_private_peer_address(&self, device: &PublicKey, address: Vec<u8>) -> Result<()> {
        if address.is_empty() || address.len() > 4096 {
            return Err(invalid("peer address length"));
        }
        let _guard = self.identity_lock.lock().expect("identity write lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        self.require_private_sender(device, None)?;
        self.query_with_params("?[device_pubkey, address] <- [[$device, $address]] :put self_peer_address {device_pubkey => address}", BTreeMap::from([
            ("device".into(), DataValue::Bytes(device.as_bytes().to_vec())),
            ("address".into(), DataValue::Bytes(address)),
        ]))?;
        Ok(())
    }

    /// Return bounded address hints; every network dial rechecks authorization.
    pub fn private_peer_addresses(&self) -> Result<Vec<(PublicKey, Vec<u8>)>> {
        self.query("?[device_pubkey, address] := *self_peer_address{device_pubkey, address}")?
            .rows
            .iter()
            .map(|row| {
                Ok((
                    PublicKey::from_bytes(
                        bytes(&row[0])?
                            .try_into()
                            .map_err(|_| invalid("device key length"))?,
                    ),
                    bytes(&row[1])?.to_vec(),
                ))
            })
            .collect()
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "private transport configuration",
        message: message.into(),
    }
}

fn serialization(error: carrel_core::identity::SerializeError) -> StoreError {
    StoreError::Signing {
        context: "paired identity material",
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{EditNoteInput, NoteRecord, PutNoteInput};
    use carrel_core::sharing::ContactCard;

    struct Recovery {
        store: Store,
        master: Keypair,
        original: DeviceCert,
        intermediate: Keypair,
        fresh: Keypair,
        note: NoteRecord,
        grant: PairedIdentity,
    }

    impl Recovery {
        fn new() -> Self {
            let store = Store::open_in_memory().unwrap();
            store.migrate().unwrap();
            let master = Keypair::generate();
            let original_key = Keypair::generate();
            let intermediate = Keypair::generate();
            let fresh = Keypair::generate();
            let original = DeviceCert::new_at_micros(
                &master,
                original_key.public(),
                10,
                Some("Original desktop".into()),
            );
            let document = PrivateDocument {
                namespace: [1; 32],
                capability: vec![2],
            };
            store
                .bind_paired_identity(
                    &PairedIdentity {
                        certificate: original.clone(),
                        document: document.clone(),
                        certificates: Vec::new(),
                        revocations: Vec::new(),
                        peers: Vec::new(),
                    },
                    &original_key,
                )
                .unwrap();
            store
                .prepare_restored_archive(Some(*original_key.public()), &intermediate)
                .unwrap();
            let note = store
                .put_note(
                    PutNoteInput {
                        body: "Written while the first recovered desktop was unbound".into(),
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
            let grant = PairedIdentity {
                certificate: DeviceCert::for_recovery(&master, fresh.public()),
                document,
                certificates: vec![original.clone()],
                revocations: Vec::new(),
                peers: Vec::new(),
            };
            Self {
                store,
                master,
                original,
                intermediate,
                fresh,
                note,
                grant,
            }
        }

        fn author_certificate(&self) -> DeviceCert {
            DeviceCert::new_at_micros(
                &self.master,
                self.intermediate.public(),
                15,
                Some("Intermediate desktop".into()),
            )
        }

        fn assert_unbound(&self) {
            assert!(self.store.self_identity().unwrap().is_none());
            assert!(
                self.store
                    .device_certificate(self.master.public(), self.fresh.public())
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                self.store
                    .device_certificate(self.master.public(), &self.original.device_pubkey)
                    .unwrap(),
                Some(self.original.clone())
            );
            assert!(
                self.store
                    .annotation_creator_permissions(self.fresh.public())
                    .unwrap()
                    .contains(self.intermediate.public())
            );
        }

        fn edit(&self, note: &NoteRecord) -> NoteRecord {
            self.store
                .edit_note(
                    EditNoteInput {
                        id: note.id.clone(),
                        body: format!("{} — edited", note.body),
                        expected_revision_id: note.revision_id.clone(),
                        now_micros: Some(note.updated_at_micros + 1),
                    },
                    &self.fresh,
                )
                .unwrap()
        }
    }

    #[test]
    fn recovery_pairing_keeps_uncertified_authors_editable_until_a_verified_grant_covers_them() {
        let mut recovery = Recovery::new();
        let before = recovery
            .store
            .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
            .unwrap()
            .rows;
        assert!(matches!(
            recovery.store.bind_paired_identity(&recovery.grant, &recovery.fresh),
            Err(StoreError::InvalidInput { context: "archive recovery", message })
                if message.contains("master authorization is required")
        ));
        recovery.assert_unbound();
        assert_eq!(
            recovery
                .store
                .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
                .unwrap()
                .rows,
            before
        );
        let local_edit = recovery.edit(&recovery.note);
        local_edit.verify_signature().unwrap();
        recovery
            .grant
            .certificates
            .push(recovery.author_certificate());
        recovery
            .store
            .bind_paired_identity(&recovery.grant, &recovery.fresh)
            .unwrap();
        assert_eq!(
            recovery.store.private_document().unwrap(),
            Some(recovery.grant.document.clone())
        );
        assert_eq!(
            recovery
                .store
                .device_certificate(recovery.master.public(), &recovery.original.device_pubkey)
                .unwrap(),
            Some(recovery.original.clone())
        );
        let bound_edit = recovery.edit(&local_edit);
        bound_edit.verify_signature().unwrap();
        assert_eq!(bound_edit.created_by, *recovery.intermediate.public());

        let peer = Store::open_in_memory().unwrap();
        peer.migrate().unwrap();
        let peer_key = Keypair::generate();
        peer.bind_self_identity(
            &DeviceCert::new(&recovery.master, peer_key.public(), None),
            &peer_key,
        )
        .unwrap();
        peer.accept_self_device_certificate(&recovery.grant.certificate)
            .unwrap();
        peer.accept_self_device_certificate(&recovery.author_certificate())
            .unwrap();
        for note in [&recovery.note, &local_edit, &bound_edit] {
            peer.accept_self_note_revision(note, recovery.fresh.public())
                .unwrap();
        }
        assert_eq!(peer.get_note(&bound_edit.id).unwrap(), Some(bound_edit));
    }

    #[test]
    fn recovery_pairing_accepts_retained_author_certificates_without_reissuing_them() {
        let mut recovery = Recovery::new();
        let certificate = recovery.author_certificate();
        recovery.store.query_with_params(
            "?[master_pubkey,device_pubkey,device_cert] <- [[$master,$device,$cert]] :put device_certificate {master_pubkey,device_pubkey=>device_cert}",
            crate::identity::certificate_params(&certificate).unwrap(),
        ).unwrap();
        recovery.grant.certificates.clear();
        recovery
            .store
            .bind_paired_identity(&recovery.grant, &recovery.fresh)
            .unwrap();
        assert_eq!(
            recovery
                .store
                .device_certificate(recovery.master.public(), recovery.intermediate.public())
                .unwrap(),
            Some(certificate)
        );
        assert_eq!(
            recovery
                .store
                .device_certificate(recovery.master.public(), &recovery.original.device_pubkey)
                .unwrap(),
            Some(recovery.original.clone())
        );
        recovery.edit(&recovery.note).verify_signature().unwrap();
    }

    #[test]
    fn recovery_pairing_rejects_wrong_root_forged_and_conflicting_grant_certificates() {
        for invalid_kind in 0..3 {
            let mut recovery = Recovery::new();
            let mut author = recovery.author_certificate();
            match invalid_kind {
                0 => {
                    author =
                        DeviceCert::new(&Keypair::generate(), recovery.intermediate.public(), None);
                }
                1 => author.authorized_at_micros += 1,
                _ => {
                    recovery.grant.certificates[0] = DeviceCert::new_at_micros(
                        &recovery.master,
                        &recovery.original.device_pubkey,
                        99,
                        Some("Replacement certificate".into()),
                    );
                }
            }
            recovery.grant.certificates.push(author);
            let result = recovery
                .store
                .bind_paired_identity(&recovery.grant, &recovery.fresh);
            match invalid_kind {
                0 => assert!(matches!(result, Err(StoreError::IdentityMasterMismatch))),
                1 => assert!(result.is_err()),
                _ => assert!(matches!(
                    result,
                    Err(StoreError::IdentityCertificateConflict)
                )),
            }
            recovery.assert_unbound();
            recovery.edit(&recovery.note).verify_signature().unwrap();
        }
    }

    #[test]
    fn recovery_pairing_retains_revoked_and_blocked_author_proofs_without_operational_access() {
        for revoked in [false, true] {
            let mut recovery = Recovery::new();
            recovery
                .grant
                .certificates
                .push(recovery.author_certificate());
            if revoked {
                recovery
                    .grant
                    .revocations
                    .push(RevocationCert::new_at_micros(
                        &recovery.master,
                        recovery.intermediate.public(),
                        i64::MAX - 1,
                    ));
            } else {
                recovery.store.query_with_params(
                    "?[master_pubkey,device_pubkey,reason] <- [[$master,$device,'local denial']] :put device_local_block {master_pubkey,device_pubkey=>reason}",
                    BTreeMap::from([
                        ("master".into(), DataValue::Bytes(recovery.master.public().as_bytes().to_vec())),
                        ("device".into(), DataValue::Bytes(recovery.intermediate.public().as_bytes().to_vec())),
                    ]),
                ).unwrap();
            }
            recovery
                .store
                .bind_paired_identity(&recovery.grant, &recovery.fresh)
                .unwrap();
            assert!(
                !recovery
                    .store
                    .self_device_authorized(recovery.intermediate.public())
                    .unwrap()
            );
            assert!(
                recovery
                    .store
                    .self_device_authorized(recovery.fresh.public())
                    .unwrap()
            );
            assert_eq!(
                recovery
                    .store
                    .device_certificate(recovery.master.public(), recovery.intermediate.public())
                    .unwrap(),
                Some(recovery.author_certificate())
            );
            recovery.edit(&recovery.note).verify_signature().unwrap();
        }
    }

    #[test]
    fn recovery_reconnect_refuses_the_master_key_as_an_operational_device() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let master = Keypair::generate();
        let device = Keypair::generate();
        store
            .bind_self_identity(&DeviceCert::new(&master, device.public(), None), &device)
            .unwrap();
        let card = ContactCard::sign(
            DeviceCert::new(&master, master.public(), None),
            "master endpoint".into(),
            &master,
        )
        .unwrap();
        card.verify().unwrap();
        let before = store
            .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
            .unwrap()
            .rows;
        assert!(matches!(
            store.register_recovered_desktop(&card),
            Err(StoreError::IdentityDeviceUnauthorized)
        ));
        assert!(
            store
                .device_certificate(master.public(), master.public())
                .unwrap()
                .is_none()
        );
        assert!(store.private_peer_addresses().unwrap().is_empty());
        assert_eq!(
            store
                .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
                .unwrap()
                .rows,
            before
        );
    }
}
