//! Explicit local recovery ownership never authorizes an incoming network peer.
use crate::{Result, Store, StoreError};
use carrel_core::{
    identity::{DeviceCert, Keypair, PublicKey},
    private_library::PrivateFact,
};
use cozo::DataValue;
use std::collections::{BTreeMap, BTreeSet};

/// Provenance and the independently generated key of a recovered installation.
#[derive(Clone, Debug)]
pub struct RestoreOrigin {
    /// Source identity, absent when the archive never enabled sync.
    pub master_pubkey: Option<PublicKey>,
    /// Only this fresh key receives the local ownership grant.
    pub device_pubkey: PublicKey,
    /// Personal author keys retained without rewriting their signatures.
    pub legacy_devices: Vec<PublicKey>,
}

impl Store {
    /// Include history, tombstones and configuration in restore eligibility.
    pub fn is_empty_for_restore(&self) -> Result<bool> {
        for row in self.query("::relations")?.rows {
            let Some(DataValue::Str(name)) = row.first() else {
                return Err(invalid("invalid relation catalog"));
            };
            if name.as_str() == "schema_version" || name.contains(':') {
                continue;
            }
            if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(invalid("invalid relation name"));
            }
            if !self
                .query(&format!("?[present] := *{name}{{}}, present=true :limit 1"))?
                .rows
                .is_empty()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Read explicit local recovery provenance, never inferred from peers.
    pub fn restore_origin(&self) -> Result<Option<RestoreOrigin>> {
        let rows=self.query("?[master,device,legacy] := *restore_origin{singleton:0,master_pubkey:master,device_pubkey:device,legacy_devices:legacy}")?;
        rows.rows
            .first()
            .map(|row| {
                let master_pubkey = if row[0] == DataValue::Null {
                    None
                } else {
                    Some(key(&row[0])?)
                };
                let DataValue::List(legacy) = &row[2] else {
                    return Err(invalid("invalid recovery authors"));
                };
                if legacy.len() > 512 {
                    return Err(invalid("too many recovery authors"));
                }
                Ok(RestoreOrigin {
                    master_pubkey,
                    device_pubkey: key(&row[1])?,
                    legacy_devices: legacy.iter().map(key).collect::<Result<_>>()?,
                })
            })
            .transpose()
    }

    /// Identify the local reader only for retained conversation and ancestry reads.
    /// Recovery provenance grants no operational, consent or peer authorization.
    pub fn local_conversation_history_master(&self) -> Result<Option<PublicKey>> {
        if let Some(binding) = self.self_identity()? {
            return Ok(Some(binding.master_pubkey));
        }
        Ok(self
            .restore_origin()?
            .and_then(|origin| origin.master_pubkey))
    }

    /// Prepare an isolated restored store; never call against an active installation.
    pub(crate) fn prepare_restored_archive(
        &self,
        source_device: Option<PublicKey>,
        fresh: &Keypair,
    ) -> Result<RestoreOrigin> {
        let binding = self.self_identity()?;
        let previous = self.restore_origin()?;
        if binding.is_some_and(|binding| Some(binding.device_pubkey) != source_device) {
            return Err(invalid("backup device differs from its identity"));
        }
        if let Some(binding) = binding
            && self
                .device_certificate(&binding.master_pubkey, &binding.device_pubkey)?
                .is_none()
        {
            return Err(invalid("backup identity has no verified local certificate"));
        }
        let master = binding
            .map(|b| b.master_pubkey)
            .or_else(|| previous.as_ref().and_then(|p| p.master_pubkey));
        let mut legacy = BTreeSet::new();
        legacy.extend(source_device);
        if let Some(previous) = previous {
            legacy.extend(previous.legacy_devices);
        }
        if binding.is_some() {
            legacy.extend(
                self.self_devices()?
                    .into_iter()
                    .map(|d| d.certificate.device_pubkey),
            );
        }
        if legacy.contains(fresh.public()) || master == Some(*fresh.public()) || legacy.len() > 512
        {
            return Err(invalid("recovery requires an independent device key"));
        }
        // Only isolated backup staging may clear this copied binding.
        self.query("::set_triggers self_identity")?;
        let prepared=self.query_with_params(r#"
            { ?[singleton] <- [[0]] :rm self_identity {singleton} }
            { ?[device_pubkey] := *self_peer_address{device_pubkey} :rm self_peer_address {device_pubkey} }
            { ?[singleton,master_pubkey,device_pubkey,legacy_devices] <- [[0,$master,$device,$legacy]] :put restore_origin {singleton=>master_pubkey,device_pubkey,legacy_devices} }
        "#,BTreeMap::from([("master".into(),master.map_or(DataValue::Null,value)),("device".into(),value(*fresh.public())),("legacy".into(),DataValue::List(legacy.into_iter().map(value).collect()))]));
        let guarded = self.query(crate::schema::SELF_IDENTITY_GUARDS[0]);
        prepared?;
        guarded?;
        self.restore_origin()?
            .ok_or_else(|| invalid("missing recovery provenance"))
    }

    pub(crate) fn check_recovery_binding(
        &self,
        master: &PublicKey,
        device: &PublicKey,
    ) -> Result<()> {
        if let Some(origin) = self.restore_origin()? {
            if origin.device_pubkey != *device || origin.legacy_devices.contains(device) {
                return Err(StoreError::IdentityDeviceUnauthorized);
            }
            if origin
                .master_pubkey
                .is_some_and(|expected| expected != *master)
            {
                return Err(StoreError::IdentityMasterMismatch);
            }
        }
        Ok(())
    }

    pub(crate) fn require_recovery_author_certificates(
        &self,
        master: &PublicKey,
        supplied: &BTreeSet<PublicKey>,
    ) -> Result<()> {
        if let Some(origin) = self.restore_origin()? {
            for author in origin.legacy_devices {
                // Callers verify every supplied certificate and its master first.
                // Revoked creators still need their original authorship proof.
                if self.device_certificate(master, &author)?.is_none()
                    && !supplied.contains(&author)
                {
                    return Err(invalid(
                        "master authorization is required for uncertified restored authors before binding",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Explicitly authorize the fresh device and all original unbound authors atomically.
    /// New certificates use a fixed label and time so recovery order cannot change
    /// a retained key's immutable certificate. Existing certificates stay intact.
    pub fn authorize_restored_identity(
        &self,
        master: &Keypair,
        device: &Keypair,
        _name: &str,
    ) -> Result<()> {
        let _guard = self.identity_lock.lock().expect("recovery identity lock");
        let origin = self
            .restore_origin()?
            .ok_or_else(|| invalid("archive has no recovery provenance"))?;
        if master.public() == device.public() || origin.legacy_devices.contains(master.public()) {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.check_recovery_binding(master.public(), device.public())?;
        if self.self_identity()?.is_some() {
            return Err(StoreError::IdentityBindingConflict);
        }
        if self.device_denied(master.public(), device.public())? {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let mut certificates = vec![
            self.device_certificate(master.public(), device.public())?
                .unwrap_or_else(|| DeviceCert::for_recovery(master, device.public())),
        ];
        for key in origin.legacy_devices {
            // A second backup may contain notes written by the first restore's
            // fresh key before it was authorized. Certify only those missing
            // local authors; never replace history or reverse a denial.
            if self.device_certificate(master.public(), &key)?.is_none()
                && !self.device_denied(master.public(), &key)?
            {
                certificates.push(DeviceCert::for_recovery(master, &key));
            }
        }
        let mut rows = Vec::new();
        let mut facts = Vec::new();
        for certificate in certificates {
            certificate.verify()?;
            self.require_matching_certificate(&certificate)?;
            let fact = PrivateFact::Certificate(certificate.clone());
            rows.push(DataValue::List(vec![
                value(*master.public()),
                value(certificate.device_pubkey),
                DataValue::Bytes(
                    certificate
                        .to_bytes()
                        .map_err(|_| invalid("certificate encoding"))?,
                ),
            ]));
            facts.push(DataValue::List(vec![
                DataValue::from(fact.key()?),
                DataValue::Bytes(fact.to_bytes()?),
            ]));
        }
        self.query_with_params(r#"
            { ?[master_pubkey,device_pubkey,device_cert] <- $certificates :put device_certificate {master_pubkey,device_pubkey=>device_cert} }
            { ?[key,encoded] <- $facts :put self_fact_history {key=>encoded} }
            { ?[key,encoded] <- $facts :put self_fact_outbox {key=>encoded} }
            { ?[singleton,master_pubkey,device_pubkey] <- [[0,$master,$device]] :put self_identity {singleton=>master_pubkey,device_pubkey} }
        "#,BTreeMap::from([("certificates".into(),DataValue::List(rows)),("facts".into(),DataValue::List(facts)),("master".into(),value(*master.public())),("device".into(),value(*device.public()))]))?;
        self.identity_changed();
        Ok(())
    }
}

fn key(value: &DataValue) -> Result<PublicKey> {
    let DataValue::Bytes(bytes) = value else {
        return Err(invalid("invalid recovery key"));
    };
    let bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| invalid("invalid recovery key length"))?;
    let key = PublicKey::from_bytes(bytes);
    key.validate()
        .map_err(|_| invalid("invalid recovery public key"))?;
    Ok(key)
}
fn value(key: PublicKey) -> DataValue {
    DataValue::Bytes(key.as_bytes().to_vec())
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "archive recovery",
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{EditNoteInput, PutNoteInput};
    use carrel_core::identity::RevocationCert;
    use proptest::prelude::*;

    fn deny(store: &Store, master: &Keypair, device: &PublicKey, revoked: bool) {
        let mut params = BTreeMap::from([
            ("master".into(), value(*master.public())),
            ("device".into(), value(*device)),
        ]);
        if revoked {
            let proof = RevocationCert::new_at_micros(master, device, i64::MAX - 1);
            params.insert(
                "at".into(),
                DataValue::Validity(cozo::Validity::from((proof.revoked_at_micros, true))),
            );
            params.insert("proof".into(), DataValue::Bytes(proof.to_bytes().unwrap()));
            store.query_with_params("?[master_pubkey,device_pubkey,revoked_at,revocation_cert] <- [[$master,$device,$at,$proof]] :put device_revocation {master_pubkey,device_pubkey,revoked_at=>revocation_cert}", params).unwrap();
        } else {
            store.query_with_params("?[master_pubkey,device_pubkey,reason] <- [[$master,$device,'explicit local block']] :put device_local_block {master_pubkey,device_pubkey=>reason}", params).unwrap();
        }
    }

    fn restored(master: &Keypair, original: &Keypair, fresh: &Keypair) -> Store {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(master, original.public(), 42, Some("Original".into())),
                original,
            )
            .unwrap();
        store
            .prepare_restored_archive(Some(*original.public()), fresh)
            .unwrap();
        store
    }

    #[test]
    fn conversation_history_master_never_infers_identity_from_self_peer_metadata() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let peer = Keypair::generate();
        let fresh = Keypair::generate();
        store.query_with_params(
            "?[pubkey,pet_name,self_described_name,is_self,added_at,last_seen] <- [[$key,null,null,true,$at,null]] :put peer {pubkey=>pet_name,self_described_name,is_self,added_at,last_seen}",
            BTreeMap::from([
                ("key".into(), value(*peer.public())),
                ("at".into(), DataValue::Validity(cozo::Validity::from((1, true)))),
            ]),
        ).unwrap();
        assert_eq!(store.local_conversation_history_master().unwrap(), None);
        store
            .prepare_restored_archive(Some(*peer.public()), &fresh)
            .unwrap();
        assert_eq!(store.local_conversation_history_master().unwrap(), None);
        assert!(
            store
                .item_conversations("unbound-article")
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .received_item_conversations("unbound-article")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn conversation_history_master_uses_restore_provenance_and_prefers_a_binding() {
        let master = Keypair::generate();
        let original = Keypair::generate();
        let fresh = Keypair::generate();
        let store = restored(&master, &original, &fresh);
        assert_eq!(
            store.local_conversation_history_master().unwrap(),
            Some(*master.public())
        );
        assert!(store.self_identity().unwrap().is_none());
        store
            .authorize_restored_identity(&master, &fresh, "Recovered")
            .unwrap();
        // A diagnostic provenance row cannot override the explicitly bound reader.
        let other = Keypair::generate();
        store.query_with_params(
            "?[singleton,master_pubkey,device_pubkey,legacy_devices] <- [[0,$master,$device,[]]] :put restore_origin {singleton=>master_pubkey,device_pubkey,legacy_devices}",
            BTreeMap::from([("master".into(), value(*other.public())), ("device".into(), value(*fresh.public()))]),
        ).unwrap();
        assert_eq!(
            store.local_conversation_history_master().unwrap(),
            Some(*master.public())
        );
        assert_eq!(
            store.self_identity().unwrap().unwrap().master_pubkey,
            *master.public()
        );
    }

    #[test]
    fn resumption_never_certifies_a_denied_intermediate_recovery_key() {
        for revoked in [false, true] {
            let master = Keypair::generate();
            let original = Keypair::generate();
            let intermediate = Keypair::generate();
            let fresh = Keypair::generate();
            let store = restored(&master, &original, &intermediate);
            let certificate = store
                .device_certificate(master.public(), original.public())
                .unwrap();
            deny(&store, &master, intermediate.public(), revoked);
            deny(&store, &master, original.public(), revoked);
            store
                .prepare_restored_archive(Some(*intermediate.public()), &fresh)
                .unwrap();
            store
                .authorize_restored_identity(&master, &fresh, "Recovered")
                .unwrap();
            assert!(
                store
                    .device_certificate(master.public(), intermediate.public())
                    .unwrap()
                    .is_none()
            );
            for denied in [intermediate.public(), original.public()] {
                assert!(store.device_denied(master.public(), denied).unwrap());
                assert!(!store.self_device_authorized(denied).unwrap());
            }
            assert_eq!(
                store
                    .device_certificate(master.public(), original.public())
                    .unwrap(),
                certificate
            );
            assert!(
                store
                    .annotation_creator_permissions(fresh.public())
                    .unwrap()
                    .contains(original.public())
            );
            assert!(store.self_device_authorized(fresh.public()).unwrap());
        }
    }

    #[test]
    fn recovery_resumption_refuses_a_denied_fresh_device_without_binding() {
        for revoked in [false, true] {
            let master = Keypair::generate();
            let original = Keypair::generate();
            let fresh = Keypair::generate();
            let store = restored(&master, &original, &fresh);
            deny(&store, &master, fresh.public(), revoked);
            assert!(matches!(
                store.authorize_restored_identity(&master, &fresh, "Recovered"),
                Err(StoreError::IdentityDeviceUnauthorized)
            ));
            assert!(store.self_identity().unwrap().is_none());
            assert!(
                store
                    .device_certificate(master.public(), fresh.public())
                    .unwrap()
                    .is_none()
            );
            assert!(
                store
                    .annotation_creator_permissions(fresh.public())
                    .unwrap()
                    .contains(original.public())
            );
        }
    }

    #[test]
    fn recovery_resumption_preserves_an_existing_certificate_for_its_local_key() {
        let master = Keypair::generate();
        let original = Keypair::generate();
        let fresh = Keypair::generate();
        let store = restored(&master, &original, &fresh);
        let certificate = DeviceCert::new_at_micros(
            &master,
            fresh.public(),
            77,
            Some("Previously certified".into()),
        );
        store.query_with_params(
            "?[master_pubkey,device_pubkey,device_cert] <- [[$master,$device,$cert]] :put device_certificate {master_pubkey,device_pubkey=>device_cert}",
            crate::identity::certificate_params(&certificate).unwrap(),
        ).unwrap();
        store
            .authorize_restored_identity(&master, &fresh, "Another name")
            .unwrap();
        assert_eq!(
            store
                .device_certificate(master.public(), fresh.public())
                .unwrap(),
            Some(certificate.clone())
        );
        let fact = PrivateFact::Certificate(certificate);
        assert!(
            store
                .private_fact_outbox(None, 100)
                .unwrap()
                .iter()
                .any(|(_, queued)| { queued.to_bytes().unwrap() == fact.to_bytes().unwrap() })
        );
    }

    #[test]
    fn independent_recoveries_share_certificates_for_live_and_historical_authors() {
        let master = Keypair::generate();
        let original = Keypair::generate();
        let intermediate = Keypair::generate();
        let fresh_a = Keypair::generate();
        let fresh_b = Keypair::generate();
        let first = restored(&master, &original, &intermediate);
        let second = restored(&master, &original, &intermediate);
        let third = restored(&master, &original, &intermediate);
        let input = PutNoteInput {
            body: "An intermediate author's note — 文".into(),
            target_kind: None,
            target_id: None,
            created_at_micros: Some(1),
        };
        let note = first.put_note(input.clone(), &intermediate).unwrap();
        second.put_note(input.clone(), &intermediate).unwrap();
        third.put_note(input, &intermediate).unwrap();
        // These snapshots predate resumption of the first recovery. That same
        // key will be certified live there, and historically in the later copies.
        second
            .prepare_restored_archive(Some(*intermediate.public()), &fresh_a)
            .unwrap();
        third
            .prepare_restored_archive(Some(*intermediate.public()), &fresh_b)
            .unwrap();
        for (store, device, name) in [
            (&first, &intermediate, "First desktop"),
            (&second, &fresh_a, "Second desktop"),
            (&third, &fresh_b, "Third desktop"),
        ] {
            store
                .authorize_restored_identity(&master, device, name)
                .unwrap();
        }
        let certificate = first
            .device_certificate(master.public(), intermediate.public())
            .unwrap()
            .unwrap();
        assert_eq!(certificate.authorized_at_micros, 0);
        assert_eq!(
            certificate.device_name.as_deref(),
            Some("Recovered desktop")
        );
        let original_certificate =
            DeviceCert::new_at_micros(&master, original.public(), 42, Some("Original".into()));
        for store in [&first, &second, &third] {
            let historical = store
                .device_certificate(master.public(), intermediate.public())
                .unwrap()
                .unwrap();
            assert_eq!(
                historical.to_bytes().unwrap(),
                certificate.to_bytes().unwrap()
            );
            store.accept_self_device_certificate(&certificate).unwrap();
            assert_eq!(
                store
                    .device_certificate(master.public(), original.public())
                    .unwrap(),
                Some(original_certificate.clone())
            );
        }
        let updated = first
            .edit_note(
                EditNoteInput {
                    id: note.id,
                    body: "Edited after the first recovery resumed".into(),
                    expected_revision_id: note.revision_id,
                    now_micros: Some(2),
                },
                &intermediate,
            )
            .unwrap();
        for store in [&second, &third] {
            store
                .accept_self_note_revision(&updated, intermediate.public())
                .unwrap();
            assert_eq!(store.get_note(&updated.id).unwrap(), Some(updated.clone()));
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn recovery_ownership_is_granted_only_to_its_fresh_key(old in any::<u8>(),fresh in any::<u8>(),other in any::<u8>(),root in any::<u8>()) {
            prop_assume!(old!=fresh&&old!=other&&old!=root&&fresh!=other&&fresh!=root&&other!=root);
            let original=Keypair::from_seed(&[old;32]);let fresh=Keypair::from_seed(&[fresh;32]);let outsider=Keypair::from_seed(&[other;32]);let master=Keypair::from_seed(&[root;32]);
            let store=Store::open_in_memory().unwrap();store.migrate().unwrap();store.prepare_restored_archive(Some(*original.public()),&fresh).unwrap();
            prop_assert!(store.annotation_creator_permissions(fresh.public()).unwrap().contains(original.public()));
            prop_assert!(store.annotation_creator_permissions(outsider.public()).unwrap().is_empty());
            prop_assert!(store.authorize_restored_identity(&fresh,&fresh,"Invalid").is_err());
            prop_assert!(store.authorize_restored_identity(&original,&fresh,"Invalid").is_err());
            prop_assert!(store.self_identity().unwrap().is_none());
            store.authorize_restored_identity(&master,&fresh,"Recovered").unwrap();
            prop_assert!(store.annotation_creator_permissions(fresh.public()).unwrap().contains(original.public()));
            prop_assert!(store.annotation_creator_permissions(outsider.public()).unwrap().is_empty());
            prop_assert_eq!(store.self_identity().unwrap().unwrap().master_pubkey,*master.public());
        }
    }
}
