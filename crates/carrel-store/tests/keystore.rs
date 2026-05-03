use carrel_core::identity::{Keypair, verify};
use carrel_store::keystore::{Keystore, KeystoreError};

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
