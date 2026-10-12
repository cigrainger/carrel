use std::collections::{BTreeMap, BTreeSet};

use carrel_core::library::{ItemCursor, ItemFilter, ItemView};
use carrel_store::{Store, StoreError};
use cozo::{DataValue, Validity};

fn archive(count: usize, tied: bool) -> Store {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let rows = (0..count)
        .map(|index| {
            DataValue::List(vec![
                DataValue::from(format!("item-{index:04}")),
                DataValue::from("article"),
                DataValue::from(format!("文章 {index}")),
                DataValue::List(Vec::new()),
                DataValue::Null,
                DataValue::Null,
                DataValue::Null,
                DataValue::Null,
                validity(if tied {
                    100
                } else {
                    i64::try_from(index).unwrap()
                }),
            ])
        })
        .collect();
    store.query_with_params(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- $rows
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#, BTreeMap::from([("rows".into(), DataValue::List(rows))])).unwrap();
    store
}

fn validity(time: i64) -> DataValue {
    DataValue::Validity(Validity::from((time, true)))
}

fn state(store: &Store, id: &str, state: &str, time: i64) {
    store.query_with_params(r#"
        ?[item_id, state, progress, progress_label, last_position, updated_at] <- [[$id, $state, null, null, null, $time]]
        :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
    "#, BTreeMap::from([("id".into(), DataValue::from(id)), ("state".into(), DataValue::from(state)), ("time".into(), validity(time))])).unwrap();
}

fn star(store: &Store, id: &str, starred: bool, time: i64) {
    store
        .query_with_params(
            r#"
        ?[item_id, starred, updated_at] <- [[$id, $starred, $time]]
        :put item_star {item_id => starred, updated_at}
    "#,
            BTreeMap::from([
                ("id".into(), DataValue::from(id)),
                ("starred".into(), DataValue::Bool(starred)),
                ("time".into(), validity(time)),
            ]),
        )
        .unwrap();
}

fn article(store: &Store, id: &str, feed: &str) {
    store.query_with_params(r#"
        ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] <- [[$id, $feed, 900, 4, 'A source', null]]
        :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
    "#, BTreeMap::from([("id".into(), DataValue::from(id)), ("feed".into(), DataValue::from(feed))])).unwrap();
}

fn all_ids(store: &Store, mut filter: ItemFilter) -> Vec<String> {
    let mut ids = Vec::new();
    loop {
        let page = store.list_item_page(&filter, 0).unwrap();
        ids.extend(page.items.into_iter().map(|item| item.id));
        let Some(cursor) = page.next_cursor else {
            break;
        };
        assert_ne!(filter.after.as_ref(), Some(&cursor));
        filter.after = Some(cursor);
    }
    ids
}

#[test]
fn folders_filter_before_pagination_and_follow_current_subscription_membership() {
    use carrel_core::subscription::FeedSubscription;
    let store = archive(230, true);
    for (url, folder) in [
        ("https://a.example/feed", Some("Research / 文")),
        ("https://b.example/feed", Some("Research / 文")),
        ("https://c.example/feed", Some("Research")),
        ("https://d.example/feed", None),
    ] {
        store
            .import_feed_subscriptions(&[FeedSubscription {
                url: url.into(),
                title: None,
                folder: folder.map(str::to_owned),
            }])
            .unwrap();
    }
    for i in 0..230 {
        article(
            &store,
            &format!("item-{i:04}"),
            if i < 210 {
                if i % 2 == 0 {
                    "https://a.example/feed"
                } else {
                    "https://b.example/feed"
                }
            } else {
                "https://c.example/feed"
            },
        );
    }
    let filter = ItemFilter::folder("Research / 文".into());
    let ids = all_ids(&store, filter.clone());
    assert_eq!(ids.len(), 210);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 210);
    assert_eq!(ids.first().unwrap(), "item-0000");
    assert_eq!(ids.last().unwrap(), "item-0209");
    assert_eq!(
        all_ids(&store, ItemFilter::folder("Research".into())).len(),
        20
    );
    assert!(all_ids(&store, ItemFilter::folder("missing".into())).is_empty());
    let cursor = store
        .list_item_page(&filter, 0)
        .unwrap()
        .next_cursor
        .unwrap();
    let mut wrong = ItemFilter::folder("Research".into());
    wrong.after = Some(cursor);
    assert!(store.list_item_page(&wrong, 0).is_err());
    title(&store, "item-0200", "distinctive phrase");
    assert_eq!(
        all_ids(&store, search(filter.clone(), "distinctive")),
        ["item-0200"]
    );
    store.remove_feed("https://a.example/feed").unwrap();
    assert_eq!(all_ids(&store, filter).len(), 105);
    assert_eq!(all_ids(&store, ItemFilter::library()).len(), 230);
}

fn search(mut filter: ItemFilter, query: &str) -> ItemFilter {
    filter.search = Some(query.into());
    filter
}

fn title(store: &Store, id: &str, title: &str) {
    store.query_with_params(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
            *item{id, kind, creators, primary_url, published_at, language, summary, discovered_at},
            id = $id, title = $title
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#, BTreeMap::from([("id".into(), DataValue::from(id)), ("title".into(), DataValue::from(title))])).unwrap();
}

fn content(store: &Store, id: &str, blob: &str) {
    store
        .query_with_params(
            r#"
        ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <-
            [[$id, 'html_readable', $blob, validity(1), 'test', 5]]
        :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
    "#,
            BTreeMap::from([
                ("id".into(), DataValue::from(id)),
                ("blob".into(), DataValue::from(blob)),
            ]),
        )
        .unwrap();
}

#[test]
fn search_requires_every_literal_prefix_across_title_and_current_body() {
    let store = archive(4, false);
    title(&store, "item-0000", "CAFÉ architecture");
    title(&store, "item-0001", "Café gardens");
    title(&store, "item-0002", "Architecture only");
    title(&store, "item-0003", "İSTANBUL 中文设计");
    content(&store, "item-0001", "blob-a");
    assert!(
        store
            .put_search_text("item-0001", "blob-a", "Contemporary architectural ideas")
            .unwrap()
    );
    assert_eq!(
        all_ids(&store, search(ItemFilter::library(), "cafÉ ARCH")),
        ["item-0001", "item-0000"]
    );
    assert_eq!(
        all_ids(&store, search(ItemFilter::library(), "caf café café")),
        ["item-0001", "item-0000"]
    );
    assert_eq!(
        all_ids(&store, search(ItemFilter::library(), "İST 中文")),
        ["item-0003"]
    );
    for query in [
        "***",
        "café OR unknown",
        "architecture gardens missing",
        "x' :rm item {id}",
    ] {
        assert!(all_ids(&store, search(ItemFilter::library(), query)).is_empty());
    }
    assert_eq!(
        all_ids(&store, search(ItemFilter::library(), "   ")).len(),
        4
    );
}

#[test]
fn search_filters_the_entire_archive_before_pagination() {
    let store = archive(650, false);
    store
        .query(
            r#"
        ?[item_id, state, progress, progress_label, last_position, updated_at] :=
            *item{id: item_id, discovered_at}, item_id != 'item-0000',
            state = 'read', progress = null, progress_label = null,
            last_position = null, updated_at = discovered_at
        :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
    "#,
        )
        .unwrap();
    assert_eq!(
        all_ids(&store, search(ItemFilter::unread(), "文章")),
        ["item-0000"]
    );
    let ids = all_ids(
        &store,
        search(
            ItemFilter {
                limit: Some(23),
                ..ItemFilter::library()
            },
            "文章",
        ),
    );
    assert_eq!(
        ids,
        (0..650)
            .rev()
            .map(|i| format!("item-{i:04}"))
            .collect::<Vec<_>>()
    );
    state(&store, "item-0000", "reading", 900);
    star(&store, "item-0000", true, 900);
    article(&store, "item-0000", "https://example.com/one");
    for filter in [
        ItemFilter::reading(),
        ItemFilter::starred(),
        ItemFilter::feed("https://example.com/one".into()),
    ] {
        assert_eq!(all_ids(&store, search(filter, "文章")), ["item-0000"]);
    }
    assert_eq!(
        all_ids(&store, search(ItemFilter::today(), "文章")).len(),
        650
    );
}

#[test]
fn changed_content_and_tombstones_cannot_match_stale_text() {
    let store = archive(1, false);
    content(&store, "item-0000", "old");
    assert_eq!(store.pending_search_content().unwrap().len(), 1);
    assert!(
        store
            .put_search_text("item-0000", "old", "obsolete words")
            .unwrap()
    );
    assert!(store.pending_search_content().unwrap().is_empty());
    content(&store, "item-0000", "new");
    assert!(all_ids(&store, search(ItemFilter::library(), "obsolete")).is_empty());
    assert!(
        !store
            .put_search_text("item-0000", "old", "late obsolete extraction")
            .unwrap()
    );
    assert_eq!(store.pending_search_content().unwrap()[0].blob_id, "new");
    assert!(
        store
            .put_search_text("item-0000", "new", "replacement words")
            .unwrap()
    );
    assert!(all_ids(&store, search(ItemFilter::library(), "obsolete")).is_empty());
    assert_eq!(
        all_ids(&store, search(ItemFilter::library(), "replacement")),
        ["item-0000"]
    );
    title(&store, "item-0000", "New title");
    assert!(all_ids(&store, search(ItemFilter::library(), "文章")).is_empty());
    store.query("?[id, tombstoned_at, reason] <- [['item-0000', validity(200), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    assert!(!store.put_search_text("item-0000", "new", "late").unwrap());
    assert!(store.pending_search_content().unwrap().is_empty());
    assert!(all_ids(&store, search(ItemFilter::library(), "replacement")).is_empty());
    assert!(all_ids(&store, search(ItemFilter::library(), "new")).is_empty());
}

#[test]
fn search_cursors_are_scoped_to_normalized_query_and_queries_are_bounded() {
    let store = archive(4, false);
    let mut filter = search(
        ItemFilter {
            limit: Some(1),
            ..ItemFilter::library()
        },
        "文章",
    );
    filter.after = store.list_item_page(&filter, 0).unwrap().next_cursor;
    filter.search = Some(" 文章 文章 ".into());
    assert_eq!(all_ids(&store, filter.clone()).len(), 3);
    for query in [None, Some("other".into()), Some("*".into())] {
        filter.search = query;
        assert!(matches!(
            store.list_item_page(&filter, 0),
            Err(StoreError::InvalidInput { .. })
        ));
    }
    assert!(matches!(
        store.list_item_page(&search(ItemFilter::library(), &"x".repeat(2049)), 0),
        Err(StoreError::InvalidInput { .. })
    ));
    assert!(!store.put_search_text("missing", "none", "text").unwrap());
}

#[test]
fn older_unread_items_are_filtered_before_limiting_the_archive() {
    let store = archive(650, false);
    store
        .query(
            r#"
        ?[item_id, state, progress, progress_label, last_position, updated_at] :=
            *item{id: item_id, discovered_at}, item_id != 'item-0000',
            state = 'read', progress = null, progress_label = null,
            last_position = null, updated_at = discovered_at
        :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
    "#,
        )
        .unwrap();
    let page = store.list_item_page(&ItemFilter::unread(), 0).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, "item-0000");
    assert_eq!(page.items[0].read_state, "unread");
    assert!(!page.items[0].starred);
    assert!(page.next_cursor.is_none());
}

#[test]
fn tied_timestamps_paginate_every_item_exactly_once() {
    let store = archive(1001, true);
    let filter = ItemFilter {
        limit: Some(31),
        ..ItemFilter::library()
    };
    let ids = all_ids(&store, filter);
    assert_eq!(
        ids,
        (0..1001)
            .map(|i| format!("item-{i:04}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 1001);
}

#[test]
fn all_views_join_personal_state_and_use_their_documented_order() {
    let store = archive(7, false);
    for (id, value, time) in [
        ("item-0000", "reading", 100),
        ("item-0001", "reading", 90),
        ("item-0002", "read", 80),
        ("item-0003", "unread", 70),
        ("item-0004", "abandoned", 60),
        ("item-0005", "skimmed", 50),
    ] {
        state(&store, id, value, time);
    }
    star(&store, "item-0000", true, 80);
    star(&store, "item-0001", true, 100);
    star(&store, "item-0002", false, 200);
    article(&store, "item-0000", "https://example.com/feed");
    article(&store, "item-0003", "https://example.com/feed");
    for (filter, expected) in [
        (ItemFilter::unread(), vec!["item-0006", "item-0003"]),
        (ItemFilter::reading(), vec!["item-0000", "item-0001"]),
        (ItemFilter::starred(), vec!["item-0001", "item-0000"]),
        (
            ItemFilter::feed("https://example.com/feed".into()),
            vec!["item-0003", "item-0000"],
        ),
    ] {
        assert_eq!(
            all_ids(
                &store,
                ItemFilter {
                    limit: Some(1),
                    ..filter
                }
            ),
            expected
        );
    }
    let page = store.list_item_page(&ItemFilter::reading(), 0).unwrap();
    assert_eq!(page.items[0].site_name.as_deref(), Some("A source"));
    assert_eq!(page.items[0].estimated_read_minutes, Some(4));
    assert_eq!(page.items[0].word_count, Some(900));
    assert!(page.items[0].starred);
    assert_eq!(page.items[0].discovered_at_micros, 0);
}

#[test]
fn today_uses_publication_dates_with_arrival_fallback_and_excludes_future_posts() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let day = 86_400_000_000;
    let since = 30 * day;
    let now = since + day;
    let rows = [
        ("outdated-publication-new-arrival", Some(since - 1), now),
        ("recent-published-old-arrival", Some(now - 10), since - 10),
        ("newer-published-old-arrival", Some(now - 5), since - 20),
        ("lower-bound", Some(since), now),
        ("upper-bound", Some(now), since - 20),
        ("undated-recent", None, now - 7),
        ("undated-old", None, since - 1),
        ("future-published", Some(now + 1), now),
        ("undated-future", None, now + 1),
    ]
    .into_iter()
    .map(|(id, published, arrived)| {
        DataValue::List(vec![
            DataValue::from(id),
            DataValue::from("article"),
            DataValue::from(id),
            DataValue::List(Vec::new()),
            DataValue::Null,
            published.map(validity).unwrap_or(DataValue::Null),
            DataValue::Null,
            DataValue::Null,
            validity(arrived),
        ])
    })
    .collect();
    store.query_with_params(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- $rows
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#, BTreeMap::from([("rows".into(), DataValue::List(rows))])).unwrap();
    let page = store.list_item_page(&ItemFilter::today(), since).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        [
            "upper-bound",
            "newer-published-old-arrival",
            "undated-recent",
            "recent-published-old-arrival",
            "lower-bound",
        ]
    );
    assert_eq!(
        store
            .list_item_page(&ItemFilter::library(), since)
            .unwrap()
            .items
            .len(),
        9
    );
    assert!(
        store
            .list_item_page(
                &ItemFilter {
                    search: Some("outdated".into()),
                    ..ItemFilter::today()
                },
                since
            )
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn today_preserves_its_time_window_between_pages() {
    let store = archive(6, false);
    // Publication order differs from arrival order, and the oldest two posts
    // were fetched recently but fall outside the publication window.
    store.query(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
            *item{id, kind, title, creators, primary_url, language, summary, discovered_at: arrival},
            published_at = arrival, discovered_at = validity(100 - to_int(arrival))
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#).unwrap();
    let mut filter = ItemFilter {
        limit: Some(2),
        ..ItemFilter::today()
    };
    let page = store.list_item_page(&filter, 2).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["item-0005", "item-0004"]
    );
    filter.after = page.next_cursor;
    let page = store.list_item_page(&filter, 4).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["item-0003", "item-0002"]
    );
    assert!(page.next_cursor.is_none());
}

#[test]
fn new_arrivals_do_not_shift_the_exclusive_cursor_or_duplicate_rows() {
    let store = archive(5, false);
    let mut filter = ItemFilter {
        limit: Some(2),
        ..ItemFilter::library()
    };
    let first = store.list_item_page(&filter, 0).unwrap();
    state(&store, "item-0004", "read", 500);
    store.query(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
            [['new', 'article', 'new', [], null, null, null, null, validity(99)]]
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#).unwrap();
    filter.after = first.next_cursor;
    assert_eq!(
        all_ids(&store, filter),
        ["item-0002", "item-0001", "item-0000"]
    );
}

#[test]
fn tombstones_hide_saved_items_in_every_list() {
    let store = archive(1, false);
    state(&store, "item-0000", "reading", 100);
    star(&store, "item-0000", true, 100);
    article(&store, "item-0000", "https://example.com/feed");
    store.query("?[id, tombstoned_at, reason] <- [['item-0000', validity(200), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    for filter in [
        ItemFilter::library(),
        ItemFilter::today(),
        ItemFilter::unread(),
        ItemFilter::reading(),
        ItemFilter::starred(),
        ItemFilter::feed("https://example.com/feed".into()),
    ] {
        assert!(store.list_item_page(&filter, 0).unwrap().items.is_empty());
    }
}

#[test]
fn limits_empty_archives_and_invalid_cursors_have_defined_behavior() {
    let empty = archive(0, false);
    assert!(
        empty
            .list_item_page(&ItemFilter::library(), 0)
            .unwrap()
            .items
            .is_empty()
    );
    let store = archive(102, false);
    assert_eq!(
        store
            .list_item_page(
                &ItemFilter {
                    limit: Some(0),
                    ..ItemFilter::library()
                },
                0
            )
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        store
            .list_item_page(
                &ItemFilter {
                    limit: Some(usize::MAX),
                    ..ItemFilter::library()
                },
                0
            )
            .unwrap()
            .items
            .len(),
        100
    );
    let cursor = ItemCursor {
        view: ItemView::Unread,
        sort_at_micros: 0,
        id: "x".into(),
        since_micros: None,
        search_terms: None,
    };
    assert!(matches!(
        store.list_item_page(
            &ItemFilter {
                after: Some(cursor),
                ..ItemFilter::library()
            },
            0
        ),
        Err(StoreError::InvalidInput { .. })
    ));
    let cursor = ItemCursor {
        view: ItemView::Library,
        sort_at_micros: 0,
        id: String::new(),
        since_micros: None,
        search_terms: None,
    };
    assert!(matches!(
        store.list_item_page(
            &ItemFilter {
                after: Some(cursor),
                ..ItemFilter::library()
            },
            0
        ),
        Err(StoreError::InvalidInput { .. })
    ));
}
