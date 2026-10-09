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
