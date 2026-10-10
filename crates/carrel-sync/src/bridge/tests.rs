use std::collections::BTreeMap;

use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
use carrel_core::identity::{DeviceCert, Keypair, RevocationCert, canonical_bytes};
use carrel_core::private_library::{BlobReference, PrivateHighlight};
use carrel_core::private_state::{ReadIntent, StateKey, StateKind, StateValue};
use carrel_store::highlights::{EditNoteInput, PutHighlightInput, PutNoteInput};
use carrel_store::private_library::ItemAdoption;
use iroh::{Endpoint, endpoint::presets};
use iroh_docs::api::protocol::{AddrInfoOptions, ShareMode};

use super::*;

const DEADLINE: Duration = Duration::from_secs(10);

struct Pair {
    dir: tempfile::TempDir,
    root: Keypair,
    ka: Keypair,
    kb: Keypair,
    a: SelfBridge,
    b: SelfBridge,
}

async fn host(store: Store, path: &std::path::Path, key: &Keypair) -> Arc<SelfTransport> {
    Arc::new(
        SelfTransport::start_with_builder(
            store,
            path,
            key,
            Endpoint::builder(presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap(),
    )
}

async fn pair() -> Pair {
    let dir = tempfile::tempdir().unwrap();
    let root = Keypair::from_seed(&[130; 32]);
    let ka = Keypair::from_seed(&[131; 32]);
    let kb = Keypair::from_seed(&[132; 32]);
    let a = Store::open(&dir.path().join("a/source")).unwrap();
    a.migrate().unwrap();
    let b = Store::open(&dir.path().join("b/source")).unwrap();
    b.migrate().unwrap();
    let ca = DeviceCert::new_at_micros(&root, ka.public(), 1, Some("A".into()));
    let cb = DeviceCert::new_at_micros(&root, kb.public(), 1, Some("B".into()));
    a.bind_self_identity(&ca, &ka).unwrap();
    b.bind_self_identity(&cb, &kb).unwrap();
    a.accept_self_device_certificate(&cb).unwrap();
    b.accept_self_device_certificate(&ca).unwrap();
    let ahost = host(a.clone(), &dir.path().join("a/mirror"), &ka).await;
    let bhost = host(b.clone(), &dir.path().join("b/mirror"), &kb).await;
    let a = SelfBridge::create(a, BlobStore::open(dir.path().join("a/cache")), ahost)
        .await
        .unwrap();
    let ticket = a
        .doc
        .share(ShareMode::Write, AddrInfoOptions::Id)
        .await
        .unwrap();
    let b = SelfBridge::import(
        b,
        BlobStore::open(dir.path().join("b/cache")),
        bhost,
        ticket.capability,
    )
    .await
    .unwrap();
    assert_eq!(
        b.doc.get_download_policy().await.unwrap(),
        DownloadPolicy::NothingExcept(vec![])
    );
    a.doc.start_sync(vec![b.host.address()]).await.unwrap();
    b.doc.start_sync(vec![a.host.address()]).await.unwrap();
    Pair {
        dir,
        root,
        ka,
        kb,
        a,
        b,
    }
}

async fn stop(bridge: SelfBridge) {
    let SelfBridge { doc, host, .. } = bridge;
    doc.close().await.unwrap();
    drop(doc);
    Arc::try_unwrap(host)
        .ok()
        .expect("only bridge owns transport")
        .shutdown()
        .await
        .unwrap();
}

fn article(bridge: &SelfBridge, title: &str, body: &[u8]) -> (BlobId, BlobId) {
    let image = bridge.cache.put_blocking(b"cached image bytes").unwrap();
    let body = bridge.cache.put_blocking(body).unwrap();
    bridge.store.query_with_params("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [['article', 'article', $title, ['Writer'], 'https://example.com/article', validity(2), 'fr', 'Café summary', validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}",BTreeMap::from([("title".into(),title.into())])).unwrap();
    bridge.store.query_with_params("?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <- [['article', 'html_readable', $blob, validity(3), 'fixture', $size]] :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}",BTreeMap::from([
        ("blob".into(),body.to_string().into()),("size".into(), (bridge.cache.get_blocking(&body).unwrap().len() as i64).into()),
    ])).unwrap();
    (body, image)
}

fn adopt(bridge: &SelfBridge, signer: &Keypair, image: BlobId) {
    let capture = bridge
        .store
        .capture_private_item("article")
        .unwrap()
        .unwrap();
    assert!(matches!(
        bridge
            .store
            .adopt_private_item(
                capture,
                vec![BlobReference {
                    blob_id: image.to_string(),
                    byte_size: 18
                }],
                3,
                signer
            )
            .unwrap(),
        ItemAdoption::Recorded(_)
    ));
}

async fn publish_all(bridge: &SelfBridge) {
    let mut after = None;
    loop {
        let result = bridge.publish(after).await.unwrap();
        assert!(
            result.errors.is_empty(),
            "publication errors: {:?}",
            result.errors
        );
        after = result.after;
        if after.is_none() {
            break;
        }
    }
}

async fn synchronize(from: &SelfBridge, to: &SelfBridge, ready: impl Fn() -> bool) {
    publish_all(from).await;
    from.doc.start_sync(vec![to.host.address()]).await.unwrap();
    to.doc.start_sync(vec![from.host.address()]).await.unwrap();
    tokio::time::timeout(DEADLINE, async {
        loop {
            let errors = to.replay(from.host.address()).await.unwrap();
            assert!(errors.is_empty(), "incoming errors: {errors:?}");
            if ready() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("real metadata convergence");
}

#[tokio::test]
async fn actual_peers_transfer_article_cache_states_annotations_and_offline_note_conflicts() {
    let Pair {
        dir,
        root: _,
        ka,
        kb,
        a,
        b,
    } = pair().await;
    let original = b"<p>Caf\xc3\xa9 reading content</p>";
    let (body, image) = article(&a, "Café 文", original);
    adopt(&a, &ka, image);
    let highlight = a
        .store
        .put_highlight(
            PutHighlightInput {
                item_id: "article".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some(body.to_string()),
                    exact: "Café".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 4,
                }),
                location_label: Some("Opening".into()),
                color: None,
                created_at_micros: Some(4),
            },
            &ka,
        )
        .unwrap();
    let note = a
        .store
        .put_note(
            PutNoteInput {
                body: "Initial note".into(),
                target_kind: Some("highlight".into()),
                target_id: Some(highlight.id.clone()),
                created_at_micros: Some(5),
            },
            &ka,
        )
        .unwrap();
    a.store
        .write_private_state(
            StateKey {
                kind: StateKind::Star,
                target: "article".into(),
            },
            StateValue::Star(true),
            6,
            &ka,
        )
        .unwrap();
    a.store
        .write_private_state(
            StateKey {
                kind: StateKind::Read,
                target: "article".into(),
            },
            StateValue::Read(ReadIntent::Unread),
            6,
            &ka,
        )
        .unwrap();
    a.store
        .add_private_feed("https://example.com/feed", 6, &ka)
        .unwrap();
    synchronize(&a, &b, || b.store.get_note(&note.id).unwrap().is_some()).await;
    assert_eq!(
        b.store.get_item_detail("article").unwrap().unwrap().title,
        "Café 文"
    );
    assert_eq!(
        PrivateHighlight::from(&b.store.get_highlight(&highlight.id).unwrap().unwrap()),
        PrivateHighlight::from(&highlight)
    );
    assert_eq!(b.store.get_note(&note.id).unwrap(), Some(note.clone()));
    assert!(
        b.store
            .get_feed("https://example.com/feed")
            .unwrap()
            .is_some()
    );
    let cache = b.transfer_cache(None, &[a.host.address()]).await.unwrap();
    assert!(cache.changed);
    assert!(cache.errors.is_empty(), "cache errors: {:?}", cache.errors);
    assert_eq!(b.cache.get_blocking(&body).unwrap().as_ref(), original);
    assert_eq!(
        b.cache.get_blocking(&image).unwrap().as_ref(),
        b"cached image bytes"
    );
    assert!(b.store.private_blob_incoming(None, 100).unwrap().is_empty());

    // Leaving the swarm makes these independent offline edits. Their signed
    // ancestry cannot be replaced with a display-clock winner on reconnect.
    a.doc.leave().await.unwrap();
    b.doc.leave().await.unwrap();
    let aedit = a
        .store
        .edit_note(
            EditNoteInput {
                id: note.id.clone(),
                body: "A offline draft".into(),
                expected_revision_id: note.revision_id.clone(),
                now_micros: Some(i64::MAX),
            },
            &ka,
        )
        .unwrap();
    let bedit = b
        .store
        .edit_note(
            EditNoteInput {
                id: note.id.clone(),
                body: "B offline draft".into(),
                expected_revision_id: note.revision_id,
                now_micros: Some(-10),
            },
            &kb,
        )
        .unwrap();
    synchronize(&a, &b, || b.store.note_heads(&note.id).unwrap().len() == 2).await;
    synchronize(&b, &a, || a.store.note_heads(&note.id).unwrap().len() == 2).await;
    let mut expected = vec![aedit, bedit];
    expected.sort_by(|a, b| a.revision_id.cmp(&b.revision_id));
    assert_eq!(a.store.note_heads(&note.id).unwrap(), expected);
    assert_eq!(b.store.note_heads(&note.id).unwrap(), expected);

    let removal = a.store.remove_highlight(&highlight.id, &ka).unwrap();
    a.store
        .remove_private_feed("https://example.com/feed", 7, &ka)
        .unwrap();
    synchronize(&a, &b, || {
        b.store
            .get_annotation_removal(&highlight.id)
            .unwrap()
            .is_some()
            && b.store
                .get_feed("https://example.com/feed")
                .unwrap()
                .is_none()
    })
    .await;
    assert_eq!(
        b.store.get_annotation_removal(&highlight.id).unwrap(),
        Some(removal)
    );
    assert!(b.store.get_highlight(&highlight.id).unwrap().is_none());
    assert!(
        b.store
            .get_feed("https://example.com/feed")
            .unwrap()
            .is_none()
    );
    assert!(
        b.store
            .list_notes_for_target("highlight", &highlight.id)
            .unwrap()
            .is_empty()
    );
    stop(a).await;
    stop(b).await;
    drop(dir);
}

#[tokio::test]
async fn corrupt_unknown_oversized_and_shadowing_entries_do_not_import_raw_data() {
    let Pair {
        dir,
        root: _,
        ka,
        kb: _,
        a,
        b,
    } = pair().await;
    let (_, image) = article(&a, "Authentic", b"<p>Source</p>");
    adopt(&a, &ka, image);
    publish_all(&a).await;
    let records = a.store.private_item_heads("article").unwrap();
    let fact = PrivateFact::Item(records[0].clone());
    let key = format!("fact/{}", fact.key().unwrap());
    let mut forged = records[0].clone();
    forged.item.title = "forged".into();
    a.doc
        .set_bytes(
            a.author,
            format!("fact/item/{}", "0".repeat(64)),
            canonical_bytes(&PrivateFact::Item(forged)).unwrap(),
        )
        .await
        .unwrap();
    // A different valid SDK author with the same key must not hide the valid
    // application's payload from another author in the all-author replay.
    let unknown = a.host.docs().author_create().await.unwrap();
    a.doc
        .set_bytes(unknown, key.clone(), b"raw database write".to_vec())
        .await
        .unwrap();
    let oversized_key = format!("fact/note/{}", "1".repeat(64));
    a.doc
        .set_hash(
            a.author,
            oversized_key.clone(),
            Hash::from_bytes([9; 32]),
            MAX_PRIVATE_FACT_BYTES as u64 + 1,
        )
        .await
        .unwrap();
    a.doc.start_sync(vec![b.host.address()]).await.unwrap();
    b.doc.start_sync(vec![a.host.address()]).await.unwrap();
    tokio::time::timeout(DEADLINE, async {
        loop {
            let _errors = b.replay(a.host.address()).await.unwrap();
            if b.store.get_item_detail("article").unwrap().is_some()
                && b.doc
                    .get_one(Query::all().author(unknown).key_exact(key.clone()))
                    .await
                    .unwrap()
                    .is_some()
                && b.doc
                    .get_one(Query::all().key_exact(oversized_key.clone()))
                    .await
                    .unwrap()
                    .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        b.store.get_item_detail("article").unwrap().unwrap().title,
        "Authentic"
    );
    let entries = b.doc.get_many(Query::all().key_exact(key)).await.unwrap();
    let mut entries = pin!(entries);
    let mut saw_untrusted = false;
    while let Some(entry) = poll_fn(|cx| entries.as_mut().poll_next(cx)).await {
        let entry = entry.unwrap();
        if entry.author() == unknown {
            assert!(matches!(
                b.receive(&entry, a.host.address()).await,
                Err(SyncError::DeviceUnauthorized)
            ));
            assert!(
                b.host
                    .blobs()
                    .get_bytes(entry.content_hash())
                    .await
                    .is_err()
            );
            saw_untrusted = true;
        }
    }
    assert!(saw_untrusted);
    let entry = b
        .doc
        .get_one(Query::all().key_exact(oversized_key))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        b.receive(&entry, a.host.address()).await,
        Err(SyncError::InvalidEntry)
    ));
    assert!(
        b.host
            .blobs()
            .get_bytes(entry.content_hash())
            .await
            .is_err()
    );
    stop(a).await;
    stop(b).await;
    drop(dir);
}

#[tokio::test]
async fn content_size_is_checked_before_accepting_bytes_and_revocation_blocks_transfers() {
    let Pair {
        dir,
        root,
        ka,
        kb: _,
        a,
        b,
    } = pair().await;
    let data = b"Only the declared bytes";
    let tag = a.host.blobs().add_slice(data).await.unwrap();
    assert!(matches!(
        b.download(a.host.address(), tag.hash, 1, MAX_READING_BLOB_BYTES)
            .await,
        Err(SyncError::BlobIntegrity)
    ));
    assert!(b.host.blobs().get_bytes(tag.hash).await.is_err());
    assert_eq!(
        b.download(
            a.host.address(),
            tag.hash,
            data.len() as u64,
            MAX_READING_BLOB_BYTES
        )
        .await
        .unwrap(),
        data
    );
    b.store
        .accept_self_revocation(&RevocationCert::new_at_micros(&root, ka.public(), i64::MAX))
        .unwrap();
    b.host.refresh_authorization().await.unwrap();
    assert!(
        b.download(
            a.host.address(),
            tag.hash,
            data.len() as u64,
            MAX_READING_BLOB_BYTES
        )
        .await
        .is_err()
    );
    stop(a).await;
    stop(b).await;
    drop(dir);
}
