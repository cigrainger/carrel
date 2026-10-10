use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_store::{
    Store,
    private_transport::{PairedIdentity, PrivateDocument},
};

fn setup() -> (PairedIdentity, Keypair, Keypair) {
    let root = Keypair::from_seed(&[190; 32]);
    let device = Keypair::from_seed(&[191; 32]);
    let peer = Keypair::from_seed(&[192; 32]);
    let certificate = DeviceCert::new_at_micros(&root, device.public(), 1, Some("B".into()));
    let peer_cert = DeviceCert::new_at_micros(&root, peer.public(), 1, Some("A".into()));
    (
        PairedIdentity {
            certificate: certificate.clone(),
            document: PrivateDocument {
                namespace: [8; 32],
                capability: vec![9],
            },
            certificates: vec![certificate, peer_cert],
            revocations: vec![],
            peers: vec![(*peer.public(), vec![1, 2, 3])],
        },
        device,
        root,
    )
}

#[test]
fn failed_pairing_rolls_back_identity_proofs_document_addresses_and_fact_journal() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let (setup, device, _) = setup();
    let mut changes = store.subscribe_identity_changes();
    store.query("::set_triggers self_document on put { ?[slot] := _new[slot, namespace, capability] :assert none }").unwrap();
    assert!(store.bind_paired_identity(&setup, &device).is_err());
    assert!(store.self_identity().unwrap().is_none());
    assert!(store.private_document().unwrap().is_none());
    assert!(store.private_peer_addresses().unwrap().is_empty());
    assert!(store.private_fact_outbox(None, 100).unwrap().is_empty());
    assert!(
        store
            .query("?[key] := *device_certificate{device_pubkey: key}")
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(!changes.has_changed().unwrap());
    for guard in carrel_store::schema::PRIVATE_LIBRARY_GUARDS {
        store.query(guard).unwrap();
    }
    store.bind_paired_identity(&setup, &device).unwrap();
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    assert_eq!(
        store.private_document().unwrap(),
        Some(setup.document.clone())
    );
    assert_eq!(store.self_devices().unwrap().len(), 2);
    assert_eq!(store.private_fact_outbox(None, 100).unwrap().len(), 2);
    store.bind_paired_identity(&setup, &device).unwrap();
    let mut changed = setup.document.clone();
    changed.namespace[0] ^= 1;
    assert!(store.bind_private_document(&changed).is_err());
    assert_eq!(store.private_document().unwrap(), Some(setup.document));
}

#[test]
fn pairing_rejects_foreign_proofs_revoked_local_keys_and_uncertified_addresses_before_binding() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let (mut setup, device, root) = setup();
    let future = Keypair::from_seed(&[193; 32]);
    let foreign_root = Keypair::from_seed(&[194; 32]);
    setup.revocations.push(RevocationCert::new_at_micros(
        &foreign_root,
        future.public(),
        1,
    ));
    assert!(store.bind_paired_identity(&setup, &device).is_err());
    assert!(store.self_identity().unwrap().is_none());
    setup.revocations = vec![RevocationCert::new_at_micros(
        &root,
        device.public(),
        i64::MAX,
    )];
    assert!(store.bind_paired_identity(&setup, &device).is_err());
    setup.revocations = vec![RevocationCert::new_at_micros(
        &root,
        future.public(),
        i64::MAX,
    )];
    setup.peers.push((*future.public(), vec![2]));
    assert!(store.bind_paired_identity(&setup, &device).is_err());
    setup.peers.pop();
    store.bind_paired_identity(&setup, &device).unwrap();
    assert_eq!(store.self_revocation_proofs().unwrap(), setup.revocations);
    assert!(!store.self_device_authorized(future.public()).unwrap());
    let future_certificate =
        DeviceCert::new_at_micros(&root, future.public(), i64::MAX, Some("Late".into()));
    store
        .accept_self_device_certificate(&future_certificate)
        .unwrap();
    assert!(!store.self_device_authorized(future.public()).unwrap());
    assert!(
        store
            .set_private_peer_address(future.public(), vec![1])
            .is_err()
    );
}
