use carrel_core::subscription::FeedSubscription;
use carrel_store::Store;

#[test]
fn batch_import_is_idempotent_deduplicates_canonical_urls_and_preserves_settings() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store
        .import_feed_subscriptions(&[FeedSubscription {
            url: "https://example.com/feed".into(),
            title: Some("My title".into()),
            folder: Some("My folder".into()),
        }])
        .unwrap();
    store.query(r#"
        ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] <-
            [['https://example.com/feed', 'My title', 'Description', validity(42), 'yesterday', 'etag', 9000, 3, 'My folder', true]]
        :put feed {url => title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}
    "#).unwrap();
    let original = store.get_feed("https://example.com/feed").unwrap().unwrap();
    let incoming = [
        FeedSubscription {
            url: "https://EXAMPLE.com:443/feed".into(),
            title: Some("Imported title".into()),
            folder: Some("Imported folder".into()),
        },
        FeedSubscription {
            url: "https://new.example/feed".into(),
            title: Some("New title".into()),
            folder: Some("Reading / Essays".into()),
        },
        FeedSubscription {
            url: "https://new.example/feed".into(),
            title: Some("Duplicate".into()),
            folder: None,
        },
    ];
    store.import_feed_subscriptions(&incoming).unwrap();
    store.import_feed_subscriptions(&incoming).unwrap();
    assert_eq!(store.list_feeds().unwrap().len(), 2);
    assert_eq!(
        store.get_feed("https://example.com/feed").unwrap().unwrap(),
        original
    );
    assert_eq!(
        store.add_feed("https://example.com/feed").unwrap(),
        original
    );
    let new = store.get_feed("https://new.example/feed").unwrap().unwrap();
    assert_eq!(new.title.as_deref(), Some("New title"));
    assert_eq!(new.folder.as_deref(), Some("Reading / Essays"));
    assert_eq!(new.last_fetched_micros, None);
    assert!(!new.auto_mark_read);
    let portable = store.feed_subscriptions().unwrap();
    assert_eq!(portable[0].title, original.title);
    assert_eq!(portable[0].folder, original.folder);
}

#[test]
fn invalid_url_anywhere_in_a_batch_prevents_all_writes() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store.add_feed("https://existing.example/feed").unwrap();
    let original = store.list_feeds().unwrap();
    for url in ["relative", "file:///private/data", ""] {
        let batch = [
            FeedSubscription {
                url: "https://valid.example/feed".into(),
                title: None,
                folder: None,
            },
            FeedSubscription {
                url: url.into(),
                title: None,
                folder: None,
            },
        ];
        assert!(store.import_feed_subscriptions(&batch).is_err());
        assert_eq!(store.list_feeds().unwrap(), original);
    }
    store.import_feed_subscriptions(&[]).unwrap();
    assert_eq!(store.list_feeds().unwrap(), original);
}
