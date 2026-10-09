use carrel_core::identity::{Keypair, verify};
use carrel_store::keystore::{Keystore, KeystoreError};

#[test]
fn master_key_from_previous_crypto_versions_still_decrypts() {
    // Generated with argon2 0.5.3 and chacha20poly1305 0.10.1 using a public
    // test seed, salt, nonce, and passphrase. This checks persisted v1 data,
    // rather than only round-tripping with the current implementation.
    let tempdir = tempfile::tempdir().unwrap();
    std::fs::write(
        tempdir.path().join("master.enc"),
        include_str!("fixtures/master-v1-argon2-0.5.json"),
    )
    .unwrap();
    let keystore = Keystore::open(tempdir.path());
    let loaded = keystore.load_master("legacy fixture passphrase").unwrap();
    let expected = Keypair::from_seed(&[11_u8; 32]);

    assert_eq!(loaded.public(), expected.public());
    assert_eq!(loaded.to_seed_bytes(), expected.to_seed_bytes());
    assert!(matches!(
        keystore.load_master("wrong"),
        Err(KeystoreError::WrongPassphrase)
    ));
}

#[test]
fn master_key_round_trips_with_passphrase() {
    let tempdir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open(tempdir.path());
    let keypair = Keypair::from_seed(&[11_u8; 32]);

    keystore
        .save_master(&keypair, "correct horse battery staple")
        .unwrap();

    let loaded = keystore
        .load_master("correct horse battery staple")
        .unwrap();
    assert_eq!(loaded.public(), keypair.public());

    let signature = loaded.sign(b"loaded master can sign");
    verify(keypair.public(), b"loaded master can sign", &signature).unwrap();
}

#[test]
fn wrong_master_passphrase_is_typed_error() {
    let tempdir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open(tempdir.path());
    let keypair = Keypair::from_seed(&[12_u8; 32]);

    keystore.save_master(&keypair, "right").unwrap();

    let err = keystore.load_master("wrong").unwrap_err();
    assert!(matches!(err, KeystoreError::WrongPassphrase));
}

#[test]
fn device_key_round_trips_plain() {
    let tempdir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open(tempdir.path());
    let keypair = Keypair::from_seed(&[13_u8; 32]);

    keystore.save_device(&keypair).unwrap();

    let loaded = keystore.load_device().unwrap();
    assert_eq!(loaded.public(), keypair.public());

    let signature = loaded.sign(b"loaded device can sign");
    verify(keypair.public(), b"loaded device can sign", &signature).unwrap();
}

#[test]
fn initial_key_creation_never_overwrites_existing_keys_or_corrupt_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let keys = Keystore::open(tempdir.path());
    let original = Keypair::from_seed(&[14; 32]);
    let replacement = Keypair::from_seed(&[15; 32]);
    keys.create_master(&original, "original").unwrap();
    keys.create_device(&original).unwrap();
    assert!(matches!(
        keys.create_master(&replacement, "replacement"),
        Err(KeystoreError::KeyAlreadyExists { .. })
    ));
    assert!(matches!(
        keys.create_device(&replacement),
        Err(KeystoreError::KeyAlreadyExists { .. })
    ));
    assert_eq!(
        keys.load_master("original").unwrap().public(),
        original.public()
    );
    assert_eq!(keys.load_device().unwrap().public(), original.public());
    let corrupt = b"interrupted old key";
    std::fs::write(tempdir.path().join("device.key"), corrupt).unwrap();
    assert!(matches!(
        keys.create_device(&replacement),
        Err(KeystoreError::KeyAlreadyExists { .. })
    ));
    assert_eq!(
        std::fs::read(tempdir.path().join("device.key")).unwrap(),
        corrupt
    );
    assert!(matches!(
        keys.load_device(),
        Err(KeystoreError::ParseKeyFile { .. })
    ));
}

#[test]
fn concurrent_first_use_installs_exactly_one_complete_device_key() {
    use std::sync::{Arc, Barrier};
    let tempdir = tempfile::tempdir().unwrap();
    let keys = Keystore::open(tempdir.path());
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (1..=8)
        .map(|seed| {
            let keys = keys.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let key = Keypair::from_seed(&[seed; 32]);
                barrier.wait();
                match keys.create_device(&key) {
                    Ok(()) => Some(*key.public()),
                    Err(KeystoreError::KeyAlreadyExists { .. }) => None,
                    Err(error) => panic!("unexpected creation failure: {error}"),
                }
            })
        })
        .collect();
    let winners: Vec<_> = threads
        .into_iter()
        .filter_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(*keys.load_device().unwrap().public(), winners[0]);
    assert_eq!(std::fs::read_dir(tempdir.path()).unwrap().count(), 1);
}

#[test]
fn replacement_preserves_crypto_format_and_readable_public_header() {
    let tempdir = tempfile::tempdir().unwrap();
    let keys = Keystore::open(tempdir.path());
    let key = Keypair::from_seed(&[16; 32]);
    keys.create_master(&key, "first").unwrap();
    assert_eq!(keys.master_public_key().unwrap(), *key.public());
    keys.save_master(&key, "second").unwrap();
    assert_eq!(keys.load_master("second").unwrap().public(), key.public());
    assert!(matches!(
        keys.load_master("first"),
        Err(KeystoreError::WrongPassphrase)
    ));
    assert_eq!(keys.master_public_key().unwrap(), *key.public());
    assert_eq!(std::fs::read_dir(tempdir.path()).unwrap().count(), 1);
}

#[test]
fn key_loading_rejects_oversized_and_mismatched_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let keys = Keystore::open(tempdir.path());
    let key = Keypair::from_seed(&[17; 32]);
    keys.create_device(&key).unwrap();
    let path = tempdir.path().join("device.key");
    let mut data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    data["public_key"] = Keypair::from_seed(&[18; 32]).public().to_hex().into();
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(matches!(
        keys.load_device(),
        Err(KeystoreError::KeyMaterialMismatch { .. })
    ));
    std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(matches!(
        keys.load_device(),
        Err(KeystoreError::KeyFileTooLarge { .. })
    ));
    std::fs::write(tempdir.path().join("master.enc"), vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(matches!(
        keys.load_master("unused"),
        Err(KeystoreError::KeyFileTooLarge { .. })
    ));
    assert!(matches!(
        keys.master_public_key(),
        Err(KeystoreError::KeyFileTooLarge { .. })
    ));
}

#[cfg(unix)]
#[test]
fn key_directory_and_installed_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("keys");
    let keys = Keystore::open(&path);
    let key = Keypair::from_seed(&[19; 32]);
    keys.create_master(&key, "private").unwrap();
    keys.create_device(&key).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for name in ["master.enc", "device.key"] {
        assert_eq!(
            std::fs::metadata(path.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    keys.save_master(&key, "replacement").unwrap();
    assert_eq!(
        std::fs::metadata(path.join("master.enc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
