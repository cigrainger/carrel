use std::collections::BTreeMap;

use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_store::highlights::{EditNoteInput, PutNoteInput};
use carrel_store::{Store, StoreError, schema};
use cozo::{DataValue, Validity};
use proptest::prelude::*;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store
}

fn cert(master: &Keypair, device: &Keypair, at: i64) -> DeviceCert {
    DeviceCert::new_at_micros(master, device.public(), at, Some("Device café".into()))
}

fn note(store: &Store, device: &Keypair) -> carrel_store::highlights::NoteRecord {
    store
        .put_note(
            PutNoteInput {
                body: "Original offline note".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: Some(1),
            },
            device,
        )
        .unwrap()
}

#[test]
fn binding_is_explicit_atomic_idempotent_and_requires_the_operational_key() {
    let store = store();
    let master = Keypair::from_seed(&[1; 32]);
    let device = Keypair::from_seed(&[2; 32]);
    let other = Keypair::from_seed(&[3; 32]);
    let original = note(&store, &device);
    let certificate = cert(&master, &device, 1);
    assert_eq!(store.self_identity().unwrap(), None);
    assert!(matches!(
        store.accept_self_device_certificate(&certificate),
        Err(StoreError::IdentityUnbound)
    ));
    assert!(matches!(
        store.bind_self_identity(&certificate, &other),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(
        store
            .query("?[device] := *device_certificate{device_pubkey: device}")
            .unwrap()
            .rows
            .len(),
        0
    );
    let binding = store.bind_self_identity(&certificate, &device).unwrap();
    assert_eq!(binding.device_pubkey, *device.public());
    assert_eq!(binding.master_pubkey, *master.public());
    assert_eq!(
        store.bind_self_identity(&certificate, &device).unwrap(),
        binding
    );
    assert_eq!(store.self_devices().unwrap().len(), 1);
    assert_eq!(store.get_note(&original.id).unwrap().unwrap(), original);
    assert!(matches!(
        store.bind_self_identity(&cert(&master, &other, 2), &other),
        Err(StoreError::IdentityBindingConflict)
    ));
    assert!(matches!(
        store.bind_self_identity(&cert(&master, &master, 2), &master),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
}

#[test]
fn foreign_and_forged_certificates_and_revocations_never_gain_trust() {
    let store = store();
    let master = Keypair::from_seed(&[4; 32]);
    let device = Keypair::from_seed(&[5; 32]);
    let other = Keypair::from_seed(&[6; 32]);
    let foreign = Keypair::from_seed(&[7; 32]);
    store
        .bind_self_identity(&cert(&master, &device, 1), &device)
        .unwrap();
    assert!(matches!(
        store.accept_self_device_certificate(&cert(&foreign, &other, 1)),
        Err(StoreError::IdentityMasterMismatch)
    ));
    assert!(matches!(
        store.accept_self_revocation(&RevocationCert::new_at_micros(&foreign, device.public(), 2)),
        Err(StoreError::IdentityMasterMismatch)
    ));
    let mut forged = cert(&master, &other, 1);
    forged.device_pubkey = *foreign.public();
    assert!(matches!(
        store.accept_self_device_certificate(&forged),
        Err(StoreError::IdentityCertificate(_))
    ));
    let mut forged = RevocationCert::new_at_micros(&master, device.public(), 2);
    forged.revoked_at_micros = 3;
    assert!(matches!(
        store.accept_self_revocation(&forged),
        Err(StoreError::IdentityCertificate(_))
    ));
    assert_eq!(store.self_devices().unwrap().len(), 1);
    assert!(store.self_device_authorized(device.public()).unwrap());
    assert!(!store.self_device_authorized(other.public()).unwrap());
}

#[test]
fn certificate_replays_are_safe_and_replacements_are_rejected() {
    let store = store();
    let master = Keypair::from_seed(&[8; 32]);
    let local = Keypair::from_seed(&[9; 32]);
    let remote = Keypair::from_seed(&[10; 32]);
    store
        .bind_self_identity(&cert(&master, &local, 1), &local)
        .unwrap();
    let remote_cert = cert(&master, &remote, 2);
    for _ in 0..3 {
        store.accept_self_device_certificate(&remote_cert).unwrap();
    }
    assert!(matches!(
        store.accept_self_device_certificate(&cert(&master, &remote, 3)),
        Err(StoreError::IdentityCertificateConflict)
    ));
    assert_eq!(store.self_devices().unwrap().len(), 2);
}

#[test]
fn competing_bindings_across_store_clones_cannot_replace_each_other() {
    use std::sync::{Arc, Barrier};
    let store = store();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [31, 33]
        .into_iter()
        .map(|seed| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let master = Keypair::from_seed(&[seed; 32]);
                let device = Keypair::from_seed(&[seed + 1; 32]);
                barrier.wait();
                store.bind_self_identity(&cert(&master, &device, 1), &device)
            })
        })
        .collect();
    let outcomes: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(StoreError::IdentityBindingConflict)))
            .count(),
        1
    );
    assert_eq!(store.self_devices().unwrap().len(), 1);
    assert_eq!(
        store
            .query("?[device] := *device_certificate{device_pubkey: device}")
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn revocation_before_authorization_future_time_and_replay_cannot_reauthorize() {
    let store = store();
    let master = Keypair::from_seed(&[11; 32]);
    let local = Keypair::from_seed(&[12; 32]);
    let remote = Keypair::from_seed(&[13; 32]);
    store
        .bind_self_identity(&cert(&master, &local, 1), &local)
        .unwrap();
    let revocation = RevocationCert::new_at_micros(&master, remote.public(), i64::MAX - 1);
    store.accept_self_revocation(&revocation).unwrap();
    let remote_cert = cert(&master, &remote, 2);
    store.accept_self_device_certificate(&remote_cert).unwrap();
    assert!(!store.self_device_authorized(remote.public()).unwrap());
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, remote.public(), -1))
        .unwrap();
    store.accept_self_revocation(&revocation).unwrap();
    store.accept_self_device_certificate(&remote_cert).unwrap();
    let row = store
        .self_devices()
        .unwrap()
        .into_iter()
        .find(|row| row.certificate.device_pubkey == *remote.public())
        .unwrap();
    assert_eq!(row.revocation, Some(revocation));
    assert!(!row.is_authorized());
    assert_eq!(
        store
            .query("?[at] := *device_revocation{revoked_at: at}")
            .unwrap()
            .rows
            .len(),
        2
    );
}

#[test]
fn trusted_self_device_can_edit_a_revoked_creators_note_but_foreign_device_cannot() {
    let store = store();
    let master = Keypair::from_seed(&[14; 32]);
    let creator = Keypair::from_seed(&[15; 32]);
    let local = Keypair::from_seed(&[16; 32]);
    let foreign = Keypair::from_seed(&[17; 32]);
    let original = note(&store, &creator);
    store
        .bind_self_identity(&cert(&master, &local, 1), &local)
        .unwrap();
    let input = EditNoteInput {
        id: original.id.clone(),
        body: "Changed on another installation".into(),
        expected_updated_at_micros: 1,
        now_micros: Some(2),
    };
    assert!(matches!(
        store.edit_note(input.clone(), &local),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    store
        .accept_self_device_certificate(&cert(&master, &creator, 1))
        .unwrap();
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, creator.public(), 3))
        .unwrap();
    assert!(
        store
            .annotation_creator_permissions(local.public())
            .unwrap()
            .contains(creator.public())
    );
    let edited = store.edit_note(input, &local).unwrap();
    assert_eq!(edited.created_by, *creator.public());
    assert_eq!(edited.updated_by, *local.public());
    edited.verify_signature().unwrap();
    assert!(
        store
            .annotation_creator_permissions(foreign.public())
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .annotation_creator_permissions(creator.public())
            .unwrap()
            .is_empty()
    );
    let input = EditNoteInput {
        id: edited.id.clone(),
        body: "forbidden".into(),
        expected_updated_at_micros: 2,
        now_micros: Some(3),
    };
    assert!(matches!(
        store.edit_note(input.clone(), &foreign),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, local.public(), 4))
        .unwrap();
    assert!(matches!(
        store.edit_note(input, &local),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    assert!(matches!(
        store.put_note(
            PutNoteInput {
                body: "new".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: None
            },
            &local
        ),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(store.get_note(&edited.id).unwrap().unwrap(), edited);
}

#[test]
fn identity_guards_prevent_raw_rebinding_certificate_mutation_and_proof_erasure() {
    let store = store();
    let master = Keypair::from_seed(&[18; 32]);
    let device = Keypair::from_seed(&[19; 32]);
    store
        .bind_self_identity(&cert(&master, &device, 1), &device)
        .unwrap();
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, device.public(), 2))
        .unwrap();
    for script in [
        "?[singleton, master_pubkey, device_pubkey] := *self_identity{master_pubkey, device_pubkey}, singleton = 1 :put self_identity {singleton => master_pubkey, device_pubkey}",
        "?[singleton, master_pubkey, device_pubkey] := *self_identity{singleton, master_pubkey: device_pubkey, device_pubkey: master_pubkey} :put self_identity {singleton => master_pubkey, device_pubkey}",
        "?[singleton] <- [[0]] :rm self_identity {singleton}",
        "?[master_pubkey, device_pubkey, device_cert] := *device_certificate{master_pubkey, device_pubkey}, device_cert = decode_base64('AA==') :put device_certificate {master_pubkey, device_pubkey => device_cert}",
        "?[master_pubkey, device_pubkey] := *device_certificate{master_pubkey, device_pubkey} :rm device_certificate {master_pubkey, device_pubkey}",
        "?[master_pubkey, device_pubkey, revoked_at] := *device_revocation{master_pubkey, device_pubkey, revoked_at} :rm device_revocation {master_pubkey, device_pubkey, revoked_at}",
    ] {
        assert!(store.query(script).is_err(), "guard should reject {script}");
    }
    assert!(!store.self_device_authorized(device.public()).unwrap());
}

#[test]
fn bound_identity_and_signed_revocations_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let master = Keypair::from_seed(&[20; 32]);
    let device = Keypair::from_seed(&[21; 32]);
    let remote = Keypair::from_seed(&[22; 32]);
    let proof = RevocationCert::new_at_micros(&master, remote.public(), 3);
    {
        let store = Store::open(dir.path()).unwrap();
        store.migrate().unwrap();
        store
            .bind_self_identity(&cert(&master, &device, 1), &device)
            .unwrap();
        store
            .accept_self_device_certificate(&cert(&master, &remote, 2))
            .unwrap();
        store.accept_self_revocation(&proof).unwrap();
    }
    let store = Store::open(dir.path()).unwrap();
    store.migrate().unwrap();
    assert_eq!(
        store.self_identity().unwrap().unwrap().device_pubkey,
        *device.public()
    );
    assert!(store.self_device_authorized(device.public()).unwrap());
    assert!(!store.self_device_authorized(remote.public()).unwrap());
    assert!(
        store
            .self_devices()
            .unwrap()
            .iter()
            .any(|row| row.revocation.as_ref() == Some(&proof))
    );
}

fn legacy_row(store: &Store, cert: &DeviceCert, revoked: bool, corrupt: bool) {
    let bytes = if corrupt {
        vec![0xff]
    } else {
        cert.to_bytes().unwrap()
    };
    store.query_with_params(
        "?[master_pubkey, device_pubkey, authorized_at, revoked_at, device_name, device_cert] <- [[$master, $device, $at, $revoked, $name, $cert]] :put device_authorization {master_pubkey, device_pubkey => authorized_at, revoked_at, device_name, device_cert}",
        BTreeMap::from([
            ("master".into(), DataValue::Bytes(cert.master_pubkey.to_bytes().to_vec())),
            ("device".into(), DataValue::Bytes(cert.device_pubkey.to_bytes().to_vec())),
            ("at".into(), DataValue::Validity(Validity::from((cert.authorized_at_micros, true)))),
            ("revoked".into(), if revoked { DataValue::Validity(Validity::from((2, true))) } else { DataValue::Null }),
            ("name".into(), cert.device_name.as_deref().map_or(DataValue::Null, DataValue::from)),
            ("cert".into(), DataValue::Bytes(bytes)),
        ]),
    ).unwrap();
}

#[test]
fn migration_resumes_every_relation_stage_verifies_legacy_rows_and_preserves_unproven_blocks() {
    let master = Keypair::from_seed(&[23; 32]);
    let local = Keypair::from_seed(&[24; 32]);
    let revoked = Keypair::from_seed(&[25; 32]);
    let corrupt = Keypair::from_seed(&[26; 32]);
    let mismatch = Keypair::from_seed(&[27; 32]);
    for stage in 0..=schema::SELF_IDENTITY_RELATIONS.len() {
        let store = Store::open_in_memory().unwrap();
        store.query(schema::CREATE_SCHEMA).unwrap();
        store.query("?[version, applied_at, description] <- [[1, validity(1), 'initial schema']] :put schema_version {version => applied_at, description}").unwrap();
        // The earlier migrations run too; interrupted identity relation creation
        // must not disturb legacy signatures or select a master automatically.
        legacy_row(&store, &cert(&master, &local, 1), false, false);
        legacy_row(&store, &cert(&master, &revoked, 1), true, false);
        legacy_row(&store, &cert(&master, &corrupt, 1), true, true);
        legacy_row(&store, &cert(&master, &mismatch, 1), false, false);
        store.query_with_params("?[master_pubkey, device_pubkey, authorized_at, revoked_at, device_name, device_cert] := *device_authorization{master_pubkey, device_pubkey, revoked_at, device_name, device_cert}, device_pubkey = $device, authorized_at = validity(999) :put device_authorization {master_pubkey, device_pubkey => authorized_at, revoked_at, device_name, device_cert}", BTreeMap::from([("device".into(), DataValue::Bytes(mismatch.public().to_bytes().to_vec()))])).unwrap();
        for (_, script) in &schema::SELF_IDENTITY_RELATIONS[..stage] {
            store.query(script).unwrap();
        }
        store.migrate().unwrap();
        store.migrate().unwrap();
        assert_eq!(store.self_identity().unwrap(), None);
        assert_eq!(
            store
                .query("?[device] := *device_authorization{device_pubkey: device}")
                .unwrap()
                .rows
                .len(),
            4
        );
        store
            .bind_self_identity(&cert(&master, &local, 1), &local)
            .unwrap();
        assert_eq!(store.self_devices().unwrap().len(), 2);
        assert!(!store.self_device_authorized(revoked.public()).unwrap());
        assert!(!store.self_device_authorized(corrupt.public()).unwrap());
        assert!(!store.self_device_authorized(mismatch.public()).unwrap());
        store
            .accept_self_device_certificate(&cert(&master, &corrupt, 1))
            .unwrap();
        assert!(!store.self_device_authorized(corrupt.public()).unwrap());
        assert_eq!(
            store
                .query("?[device] := *device_revocation{device_pubkey: device}")
                .unwrap()
                .rows
                .len(),
            0
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn revocation_is_monotone_under_arbitrary_replay_order(
        at in any::<i64>(),
        replay in prop::collection::vec(any::<bool>(), 0..16),
    ) {
        let store = store();
        let master = Keypair::from_seed(&[28; 32]);
        let local = Keypair::from_seed(&[29; 32]);
        let remote = Keypair::from_seed(&[30; 32]);
        store.bind_self_identity(&cert(&master, &local, 0), &local).unwrap();
        let authorization = cert(&master, &remote, at);
        let revocation = RevocationCert::new_at_micros(&master, remote.public(), at);
        store.accept_self_revocation(&revocation).unwrap();
        store.accept_self_device_certificate(&authorization).unwrap();
        prop_assert!(!store.self_device_authorized(remote.public()).unwrap());
        for replay_certificate in replay {
            if replay_certificate { store.accept_self_device_certificate(&authorization).unwrap(); }
            else { store.accept_self_revocation(&revocation).unwrap(); }
            prop_assert!(!store.self_device_authorized(remote.public()).unwrap());
        }
    }
}
