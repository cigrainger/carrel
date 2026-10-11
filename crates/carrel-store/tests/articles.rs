use std::collections::BTreeMap;

use carrel_core::{article::ArchivedArticle, library::ItemFilter, shape::Shape};
use carrel_store::{Store, ids::id_for_external};
use cozo::DataValue;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store
}

fn article() -> ArchivedArticle {
    ArchivedArticle {
        url: "https://example.org/article".into(),
        aliases: vec!["https://example.org/old?utm_source=test#section".into()],
        title: Some("読む — an open web".into()),
        creators: vec!["文".into()],
        published_at_micros: Some(10),
        language: Some("ja".into()),
        summary: Some("A short description".into()),
        site_name: Some("Example".into()),
        byline: Some("文".into()),
        blob_id: "a".repeat(64),
        byte_size: 100,
        extracted_with: "readable-readability".into(),
        word_count: 250,
        estimated_read_minutes: 1,
        searchable_text: "Café and architectural counterforms".into(),
        shape: Shape::default(),
    }
}

#[test]
fn new_capture_is_unread_searchable_and_marked_for_private_replication() {
    let store = store();
    let archive = article();
    let saved = store.save_archived_article(&archive, 100, None).unwrap();
    assert!(!saved.existed);
    assert_eq!(saved.id, id_for_external(&archive.url));
    assert_eq!(
        store
            .article_by_urls(&["https://example.org/old?utm_medium=email".into()])
            .unwrap(),
        Some(saved.id.clone())
    );
    let mut filter = ItemFilter::library();
    filter.search = Some("café counter".into());
    let page = store.list_item_page(&filter, 0).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].read_state, "unread");
    assert!(!page.items[0].starred);
    assert_eq!(page.items[0].published_at_micros, Some(10));
    assert_eq!(page.items[0].discovered_at_micros, 100);
    assert!(store.list_feeds().unwrap().is_empty());
    // Today uses publication, not the time a link was saved.
    assert!(
        store
            .list_item_page(&ItemFilter::today(), 50)
            .unwrap()
            .items
            .is_empty()
    );
    let capture = store.capture_private_item(&saved.id).unwrap().unwrap();
    assert_eq!(capture.item.contents[0].blob.blob_id, archive.blob_id);
    assert!(capture.item.article.unwrap().feed_url.is_none());
    assert_eq!(capture.item.identifiers.len(), 2);
}

#[test]
fn repeated_capture_preserves_archive_metadata_personal_state_and_arrival() {
    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
    use carrel_core::identity::Keypair;
    use carrel_store::highlights::PutHighlightInput;
    let store = store();
    let mut archive = article();
    let saved = store.save_archived_article(&archive, 100, None).unwrap();
    let highlight = store
        .put_highlight(
            PutHighlightInput {
                item_id: saved.id.clone(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some(archive.blob_id.clone()),
                    exact: "Café".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 4,
                }),
                location_label: None,
                color: None,
                created_at_micros: Some(150),
            },
            &Keypair::from_seed(&[111; 32]),
        )
        .unwrap();
    store.query_with_params("{ ?[item_id, state, progress, progress_label, last_position, updated_at] <- [[$id, 'read', 1.0, null, null, validity(200)]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at} } { ?[item_id, starred, updated_at] <- [[$id, true, validity(200)]] :put item_star {item_id => starred, updated_at} }", BTreeMap::from([("id".into(), DataValue::from(saved.id.as_str()))])).unwrap();
    archive.title = Some("Changed title".into());
    archive.blob_id = "b".repeat(64);
    archive.searchable_text = "Replacement content".into();
    archive.aliases.push("https://example.org/another".into());
    let again = store.save_archived_article(&archive, 300, None).unwrap();
    assert!(again.existed);
    assert_eq!(again.id, saved.id);
    assert_eq!(again.title, "読む — an open web");
    let item = &store
        .list_item_page(&ItemFilter::library(), 0)
        .unwrap()
        .items[0];
    assert_eq!(item.discovered_at_micros, 100);
    assert_eq!(item.read_state, "read");
    assert!(item.starred);
    assert_eq!(
        store.list_item_highlights(&saved.id).unwrap(),
        vec![highlight]
    );
    assert_eq!(
        store
            .get_item_detail(&saved.id)
            .unwrap()
            .unwrap()
            .readable
            .unwrap()
            .blob_id,
        "a".repeat(64)
    );
    let mut filter = ItemFilter::library();
    filter.search = Some("café".into());
    assert_eq!(store.list_item_page(&filter, 0).unwrap().items.len(), 1);
    filter.search = Some("replacement".into());
    assert!(store.list_item_page(&filter, 0).unwrap().items.is_empty());
}

#[test]
fn missing_cache_recovery_preserves_a_concurrently_recovered_archive() {
    let store = store();
    let mut archive = article();
    let saved = store.save_archived_article(&archive, 100, None).unwrap();
    let missing = archive.blob_id.clone();
    archive.blob_id = "b".repeat(64);
    assert!(
        store
            .save_archived_article(&archive, 200, Some(&missing))
            .unwrap()
            .archived
    );
    archive.blob_id = "c".repeat(64);
    assert!(
        !store
            .save_archived_article(&archive, 300, Some(&missing))
            .unwrap()
            .archived
    );
    assert_eq!(
        store
            .get_item_detail(&saved.id)
            .unwrap()
            .unwrap()
            .readable
            .unwrap()
            .blob_id,
        "b".repeat(64)
    );
}

#[test]
fn feed_article_without_content_keeps_its_identity_and_source_when_filled() {
    let store = store();
    let archive = article();
    let feed_url = "https://example.org/feed";
    store
        .ingest_feed(
            feed_url,
            &carrel_core::feed::ParsedFeed {
                title: Some("Publisher".into()),
                description: None,
                language: None,
                entries: vec![carrel_core::feed::ParsedEntry {
                    feed_guid: "one".into(),
                    title: Some("Original publisher title".into()),
                    authors: vec!["Publisher author".into()],
                    url: Some(archive.url.clone()),
                    published_at_micros: Some(20),
                    language: None,
                    summary_html: None,
                    content_html: None,
                }],
            },
            &Default::default(),
        )
        .unwrap();
    let before = store
        .list_item_page(&ItemFilter::library(), 0)
        .unwrap()
        .items[0]
        .clone();
    let saved = store.save_archived_article(&archive, 300, None).unwrap();
    assert_eq!(saved.id, before.id);
    assert_eq!(saved.title, "Original publisher title");
    let capture = store.capture_private_item(&saved.id).unwrap().unwrap();
    assert_eq!(capture.item.published_at_micros, Some(20));
    assert_eq!(
        capture.item.discovered_at_micros,
        before.discovered_at_micros
    );
    assert_eq!(
        capture.item.article.unwrap().feed_url.as_deref(),
        Some(feed_url)
    );
    assert_eq!(capture.item.contents.len(), 1);
}

#[test]
fn invalid_and_conflicting_captures_do_not_write_partial_items() {
    let store = store();
    for bad in [
        "file:///tmp/a",
        "javascript:alert(1)",
        "https://user:password@example.org/a",
    ] {
        let mut archive = article();
        archive.url = bad.into();
        assert!(store.save_archived_article(&archive, 100, None).is_err());
    }
    let mut bad = article();
    bad.byte_size = -1;
    assert!(store.save_archived_article(&bad, 100, None).is_err());
    bad = article();
    bad.blob_id = "not-a-content-hash".into();
    assert!(store.save_archived_article(&bad, 100, None).is_err());
    assert!(
        store
            .list_item_page(&ItemFilter::library(), 0)
            .unwrap()
            .items
            .is_empty()
    );
    let first = store.save_archived_article(&article(), 100, None).unwrap();
    let mut second = article();
    second.url = "https://example.org/second".into();
    second.aliases.clear();
    let other = store.save_archived_article(&second, 200, None).unwrap();
    second.aliases.push("https://example.org/article".into());
    assert!(store.save_archived_article(&second, 300, None).is_err());
    assert_ne!(first.id, other.id);
    assert_eq!(
        store
            .list_item_page(&ItemFilter::library(), 0)
            .unwrap()
            .items
            .len(),
        2
    );
}

#[test]
fn transaction_failure_rolls_back_metadata_content_and_search() {
    let store = store();
    store.query("::set_triggers item_shape on put { ?[item_id] := _new[item_id, video, audio, roundup, long, short, code, math, at] :assert none }").unwrap();
    assert!(store.save_archived_article(&article(), 100, None).is_err());
    for relation in [
        "item",
        "item_identifier",
        "item_article",
        "item_content",
        "item_search",
        "self_item_dirty",
    ] {
        let count = store
            .query(&format!(
                "?[count(item_id)] := *{relation}{{{}: item_id}}",
                if matches!(relation, "item" | "self_item_dirty") {
                    "id"
                } else {
                    "item_id"
                }
            ))
            .unwrap();
        assert_eq!(count.rows[0][0], DataValue::from(0), "{relation}");
    }
}

#[test]
fn capture_does_not_reuse_a_removed_article_or_a_different_item_kind() {
    let store = store();
    let saved = store.save_archived_article(&article(), 100, None).unwrap();
    store.query_with_params("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] := *item{id: $id, title, creators, primary_url, published_at, language, summary, discovered_at}, id = $id, kind = 'book' :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}", BTreeMap::from([("id".into(), DataValue::from(saved.id.as_str()))])).unwrap();
    assert!(store.article_by_urls(&[article().url]).is_err());
    assert!(store.save_archived_article(&article(), 200, None).is_err());
    let removed = Store::open_in_memory().unwrap();
    removed.migrate().unwrap();
    let saved = removed
        .save_archived_article(&article(), 100, None)
        .unwrap();
    removed.query_with_params("?[id, tombstoned_at, reason] <- [[$id, validity(150), null]] :put entity_tombstone {id, tombstoned_at => reason}", BTreeMap::from([("id".into(), DataValue::from(saved.id.as_str()))])).unwrap();
    assert!(removed.article_by_urls(&[article().url]).is_err());
    assert!(
        removed
            .save_archived_article(&article(), 200, None)
            .is_err()
    );
}

#[test]
fn captures_replicate_through_the_existing_signed_private_library_contract() {
    use carrel_core::identity::{DeviceCert, Keypair};
    use carrel_store::private_library::{ItemAdoption, ItemReceive};
    let source = store();
    let destination = store();
    let master = Keypair::from_seed(&[101; 32]);
    let author = Keypair::from_seed(&[102; 32]);
    let recipient = Keypair::from_seed(&[103; 32]);
    for (store, local, other) in [
        (&source, &author, &recipient),
        (&destination, &recipient, &author),
    ] {
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&master, local.public(), 1, None),
                local,
            )
            .unwrap();
        store
            .accept_self_device_certificate(&DeviceCert::new_at_micros(
                &master,
                other.public(),
                1,
                None,
            ))
            .unwrap();
    }
    let saved = source.save_archived_article(&article(), 100, None).unwrap();
    let capture = source.capture_private_item(&saved.id).unwrap().unwrap();
    let ItemAdoption::Recorded(record) = source
        .adopt_private_item(capture, vec![], 200, &author)
        .unwrap()
    else {
        panic!("new signed capture")
    };
    assert_eq!(
        destination
            .accept_self_item_revision(&record, author.public())
            .unwrap(),
        ItemReceive::Applied
    );
    let item = destination.get_item_detail(&saved.id).unwrap().unwrap();
    assert_eq!(item.title, saved.title);
    assert_eq!(item.readable.unwrap().blob_id, article().blob_id);
    assert!(destination.list_feeds().unwrap().is_empty());
}
