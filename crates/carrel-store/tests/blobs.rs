use std::str::FromStr;

use carrel_store::blobs::{BlobId, BlobStore};

#[tokio::test]
async fn blobs_are_content_addressed_and_persistent() {
    let tempdir = tempfile::tempdir().unwrap();
    let first = BlobStore::open(tempdir.path());
    let id = first.put(b"readable html").await.unwrap();

    assert_eq!(id, BlobId::from_bytes(b"readable html"));
    assert!(first.has(&id));
    assert_eq!(first.get(&id).await.unwrap(), &b"readable html"[..]);

    let second = BlobStore::open(tempdir.path());
    assert!(second.has(&id));
    assert_eq!(second.get(&id).await.unwrap(), &b"readable html"[..]);

    let parsed = BlobId::from_str(&id.to_string()).unwrap();
    assert_eq!(parsed, id);
}
