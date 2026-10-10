use std::collections::BTreeMap;

use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_core::private_library::{
    BlobReference, ItemRevision, PrivateFact, PrivateHighlight, PrivateRemoval,
};
use carrel_core::private_state::{StateKey, StateKind, StateValue};
use carrel_store::highlights::{PutHighlightInput, PutNoteInput};
use carrel_store::private_library::{ItemAdoption, ItemReceive, PrivateReceive};
use carrel_store::{Store, StoreError};
use cozo::DataValue;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store
}
fn bind(store: &Store, root: &Keypair, local: &Keypair, remote: &Keypair) {
    store
        .bind_self_identity(
            &DeviceCert::new_at_micros(root, local.public(), 1, None),
            local,
        )
        .unwrap();
    store
        .accept_self_device_certificate(&DeviceCert::new_at_micros(root, remote.public(), 1, None))
        .unwrap();
}
fn source(store: &Store, id: &str, title: &str) {
    store.query_with_params("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [[$id, 'article', $title, ['文'], 'https://example.com/article', validity(2), 'fr', 'Café', validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}", BTreeMap::from([("id".into(), DataValue::from(id)),("title".into(), DataValue::from(title))])).unwrap();
}
fn adopt(store: &Store, id: &str, signer: &Keypair) -> ItemRevision {
    let capture = store.capture_private_item(id).unwrap().unwrap();
    let ItemAdoption::Recorded(record) = store
        .adopt_private_item(capture, vec![], 2, signer)
        .unwrap()
    else {
        panic!("new signed source")
    };
    *record
}
fn highlight(store: &Store, signer: &Keypair) -> PrivateHighlight {
    let record = store
        .put_highlight(
            PutHighlightInput {
                item_id: "article".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some("a".repeat(64)),
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
            signer,
        )
        .unwrap();
    PrivateHighlight::from(&record)
}

#[test]
fn a_staged_cross_article_parent_is_discarded_without_blocking_healthy_revisions() {
    let a = store();
    let b = store();
    let root = Keypair::from_seed(&[120; 32]);
    let ak = Keypair::from_seed(&[121; 32]);
    let bk = Keypair::from_seed(&[122; 32]);
    bind(&a, &root, &ak, &bk);
    bind(&b, &root, &bk, &ak);
    source(&a, "other", "Other article");
    let parent = adopt(&a, "other", &ak);
    let mut wrong_item = parent.item.clone();
    wrong_item.id = "article".into();
    let wrong = ItemRevision::sign(wrong_item, vec![parent.revision_id.clone()], 3, &ak).unwrap();
    assert_eq!(
        b.accept_self_item_revision(&wrong, ak.public()).unwrap(),
        ItemReceive::Pending
    );
    b.accept_self_item_revision(&parent, ak.public()).unwrap();
    assert!(b.accept_self_item_revision(&wrong, ak.public()).is_err());
    assert!(b.promote_pending_items().unwrap().is_empty());
    assert!(
        b.query("?[key] := *self_library_incoming{key, kind: 'item'}")
            .unwrap()
            .rows
            .is_empty()
    );
    source(&a, "article", "Healthy article");
    let healthy = adopt(&a, "article", &ak);
    assert_eq!(
        b.accept_self_item_revision(&healthy, ak.public()).unwrap(),
        ItemReceive::Applied
    );
    assert_eq!(
        b.get_item_detail("article").unwrap().unwrap().title,
        "Healthy article"
    );
}

#[test]
fn source_capture_atomic_history_projection_queue_and_exact_acknowledgement() {
    let store = store();
    let signer = Keypair::from_seed(&[76; 32]);
    source(&store, "article", "Café 文");
    store.query("?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] <- [['article', 'https://example.com/feed', 42, 1, 'Journal', 'Writer']] :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}").unwrap();
    store.query("?[item_id, scheme, value, is_canonical, discovered_at] <- [['article', 'url', 'https://example.com/article', true, validity(1)]] :put item_identifier {item_id, scheme, value => is_canonical, discovered_at}").unwrap();
    store.query_with_params("?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <- [['article', 'html_readable', $blob, validity(3), 'fixture', 12]] :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}", BTreeMap::from([("blob".into(), DataValue::from("a".repeat(64)))])).unwrap();
    let capture = store.capture_private_item("article").unwrap().unwrap();
    assert_eq!(capture.item.article.as_ref().unwrap().word_count, Some(42));
    assert_eq!(capture.item.identifiers.len(), 1);
    let ItemAdoption::Recorded(record) = store
        .adopt_private_item(
            capture,
            vec![BlobReference {
                blob_id: "b".repeat(64),
                byte_size: 7,
            }],
            3,
            &signer,
        )
        .unwrap()
    else {
        panic!("recorded")
    };
    let record = *record;
    assert!(store.dirty_private_items(None, 100).unwrap().is_empty());
    let queue = store.private_fact_outbox(None, 100).unwrap();
    assert_eq!(
        queue,
        vec![(
            PrivateFact::Item(record.clone()).key().unwrap(),
            PrivateFact::Item(record.clone())
        )]
    );
    assert_eq!(
        store.private_item_heads("article").unwrap(),
        vec![record.clone()]
    );
    store.acknowledge_private_fact(&queue[0].0).unwrap();
    assert!(store.private_fact_outbox(None, 100).unwrap().is_empty());
    store.rebuild_private_fact_outbox().unwrap();
    assert_eq!(store.private_fact_outbox(None, 100).unwrap(), queue);
    assert!(
        store
            .query("?[key] := *self_fact_history{key} :rm self_fact_history {key}")
            .is_err()
    );
    assert!(store.query("?[id, revision_id] := *self_item_revision{id, revision_id} :rm self_item_revision {id, revision_id}").is_err());
    source(&store, "article", "Café 文");
    let capture = store.capture_private_item("article").unwrap().unwrap();
    assert_eq!(
        store
            .adopt_private_item(capture, record.item.resources.clone(), 99, &signer)
            .unwrap(),
        ItemAdoption::Unchanged
    );
    assert_eq!(store.private_item_heads("article").unwrap(), vec![record]);
}

#[test]
fn a_new_source_generation_survives_a_stale_capture_and_incoming_offline_branch() {
    let root = Keypair::from_seed(&[77; 32]);
    let akey = Keypair::from_seed(&[78; 32]);
    let bkey = Keypair::from_seed(&[79; 32]);
    let a = store();
    let b = store();
    bind(&a, &root, &akey, &bkey);
    bind(&b, &root, &bkey, &akey);
    source(&a, "article", "initial");
    let initial = adopt(&a, "article", &akey);
    assert_eq!(
        b.accept_self_item_revision(&initial, akey.public())
            .unwrap(),
        ItemReceive::Applied
    );
    source(&a, "article", "first local");
    let stale = a.capture_private_item("article").unwrap().unwrap();
    source(&a, "article", "second local");
    assert_eq!(
        a.adopt_private_item(stale, vec![], 0, &akey).unwrap(),
        ItemAdoption::Stale
    );
    source(&b, "article", "remote offline");
    let remote = adopt(&b, "article", &bkey);
    assert_eq!(
        a.accept_self_item_revision(&remote, bkey.public()).unwrap(),
        ItemReceive::Pending
    );
    assert_eq!(
        a.get_item_detail("article").unwrap().unwrap().title,
        "second local"
    );
    let local = adopt(&a, "article", &akey);
    assert_eq!(local.parents, vec![initial.revision_id]);
    assert_eq!(a.promote_pending_items().unwrap(), vec!["article"]);
    b.accept_self_item_revision(&local, akey.public()).unwrap();
    assert_eq!(
        a.private_item_heads("article").unwrap(),
        b.private_item_heads("article").unwrap()
    );
    assert_eq!(
        a.get_item_detail("article").unwrap(),
        b.get_item_detail("article").unwrap()
    );
    assert!(a.dirty_private_items(None, 100).unwrap().is_empty());
}

#[test]
fn out_of_order_highlights_notes_and_removals_preserve_original_proofs() {
    let root = Keypair::from_seed(&[80; 32]);
    let akey = Keypair::from_seed(&[81; 32]);
    let bkey = Keypair::from_seed(&[82; 32]);
    let a = store();
    let b = store();
    bind(&a, &root, &akey, &bkey);
    bind(&b, &root, &bkey, &akey);
    source(&a, "article", "initial");
    let item = adopt(&a, "article", &akey);
    let highlight = highlight(&a, &akey);
    let note = a
        .put_note(
            PutNoteInput {
                body: "A complete café note".into(),
                target_kind: Some("highlight".into()),
                target_id: Some(highlight.id.clone()),
                created_at_micros: Some(5),
            },
            &akey,
        )
        .unwrap();
    for original in [
        PrivateFact::Highlight(highlight.clone()),
        PrivateFact::Note(note.clone()),
    ] {
        let portable = PrivateFact::from_bytes(&original.to_bytes().unwrap()).unwrap();
        assert_eq!(portable, original);
        assert_eq!(
            b.accept_self_private_fact(&portable, akey.public())
                .unwrap(),
            PrivateReceive::Pending
        );
    }
    b.accept_self_item_revision(&item, akey.public()).unwrap();
    assert_eq!(
        b.promote_pending_highlights().unwrap(),
        vec![highlight.id.clone()]
    );
    assert_eq!(
        b.promote_pending_notes().unwrap().applied,
        vec![note.id.clone()]
    );
    assert_eq!(
        PrivateHighlight::from(&b.get_highlight(&highlight.id).unwrap().unwrap()),
        highlight
    );
    assert_eq!(b.get_note(&note.id).unwrap(), Some(note));
    let removal = PrivateRemoval::from(&a.remove_highlight(&highlight.id, &akey).unwrap());
    let wire = PrivateFact::Removal(removal);
    assert_eq!(
        PrivateFact::from_bytes(&wire.to_bytes().unwrap()).unwrap(),
        wire
    );
    b.accept_self_private_fact(&wire, akey.public()).unwrap();
    assert!(b.get_highlight(&highlight.id).unwrap().is_none());
    assert!(
        b.list_notes_for_target("highlight", &highlight.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        b.accept_self_private_fact(&PrivateFact::Highlight(highlight), akey.public())
            .unwrap(),
        PrivateReceive::Removed
    );
    for (key, _) in b.private_fact_outbox(None, 100).unwrap() {
        b.acknowledge_private_fact(&key).unwrap();
    }
    assert_eq!(
        b.accept_self_private_fact(&wire, akey.public()).unwrap(),
        PrivateReceive::Existing
    );
    assert!(b.private_fact_outbox(None, 100).unwrap().is_empty());
}

#[test]
fn every_fact_kind_uses_root_authorization_and_revocation_is_permanent() {
    let root = Keypair::from_seed(&[83; 32]);
    let akey = Keypair::from_seed(&[84; 32]);
    let bkey = Keypair::from_seed(&[85; 32]);
    let outsider = Keypair::from_seed(&[86; 32]);
    let untrusted = Keypair::from_seed(&[88; 32]);
    let a = store();
    let b = store();
    bind(&a, &root, &akey, &bkey);
    bind(&b, &root, &bkey, &akey);
    source(&a, "article", "initial");
    let item = adopt(&a, "article", &akey);
    b.accept_self_item_revision(&item, akey.public()).unwrap();
    let star = a
        .write_private_state(
            StateKey {
                kind: StateKind::Star,
                target: "article".into(),
            },
            StateValue::Star(true),
            3,
            &akey,
        )
        .unwrap();
    let certificate =
        DeviceCert::new_at_micros(&root, outsider.public(), 3, Some("Other desktop".into()));
    let revocation = RevocationCert::new_at_micros(&root, akey.public(), i64::MAX);
    for fact in [
        PrivateFact::State(star),
        PrivateFact::Certificate(certificate),
        PrivateFact::Revocation(revocation.clone()),
    ] {
        assert_eq!(
            PrivateFact::from_bytes(&fact.to_bytes().unwrap()).unwrap(),
            fact
        );
        assert!(matches!(
            b.accept_self_private_fact(&fact, untrusted.public()),
            Err(StoreError::IdentityDeviceUnauthorized)
        ));
        b.accept_self_private_fact(&fact, akey.public()).unwrap();
    }
    assert!(!b.self_device_authorized(akey.public()).unwrap());
    assert!(matches!(
        b.accept_self_private_fact(&PrivateFact::Item(item.clone()), akey.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(
        b.accept_self_private_fact(&PrivateFact::Item(item), bkey.public())
            .unwrap(),
        PrivateReceive::Existing
    );
    let forged =
        PrivateFact::Revocation(RevocationCert::new_at_micros(&outsider, bkey.public(), 1));
    assert!(matches!(
        b.accept_self_private_fact(&forged, bkey.public()),
        Err(StoreError::IdentityMasterMismatch)
    ));
}

#[test]
fn failed_projection_keeps_source_dirty_and_does_not_publish_history() {
    let store = store();
    let key = Keypair::from_seed(&[87; 32]);
    source(&store, "article", "original");
    let capture = store.capture_private_item("article").unwrap().unwrap();
    store
        .query(
            "::set_triggers self_fact_outbox on put { ?[key] := _new[key, encoded] :assert none }",
        )
        .unwrap();
    assert!(store.adopt_private_item(capture, vec![], 2, &key).is_err());
    assert_eq!(
        store.dirty_private_items(None, 100).unwrap(),
        vec!["article"]
    );
    assert!(store.private_item_heads("article").unwrap().is_empty());
    assert!(
        store
            .query("?[key] := *self_fact_history{key}")
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        store.get_item_detail("article").unwrap().unwrap().title,
        "original"
    );
}
