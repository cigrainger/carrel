use std::collections::BTreeMap;

use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_core::private_state::{
    ReadIntent, StateKey, StateKind, StateRevision, StateValue, SubscriptionState,
};
use carrel_core::subscription::FeedSubscription;
use carrel_store::private_state::StateReceive;
use carrel_store::{Store, StoreError};
use cozo::DataValue;

struct Pair {
    a: Store,
    b: Store,
    master: Keypair,
    ka: Keypair,
    kb: Keypair,
}
fn pair() -> Pair {
    let master = Keypair::from_seed(&[61; 32]);
    let ka = Keypair::from_seed(&[62; 32]);
    let kb = Keypair::from_seed(&[63; 32]);
    let a = Store::open_in_memory().unwrap();
    a.migrate().unwrap();
    let b = Store::open_in_memory().unwrap();
    b.migrate().unwrap();
    bind(&a, &master, &ka, &kb);
    bind(&b, &master, &kb, &ka);
    item(&a, "article");
    item(&b, "article");
    Pair {
        a,
        b,
        master,
        ka,
        kb,
    }
}
fn bind(store: &Store, master: &Keypair, local: &Keypair, remote: &Keypair) {
    store
        .bind_self_identity(
            &DeviceCert::new_at_micros(master, local.public(), 1, None),
            local,
        )
        .unwrap();
    store
        .accept_self_device_certificate(&DeviceCert::new_at_micros(
            master,
            remote.public(),
            1,
            None,
        ))
        .unwrap();
}
fn item(store: &Store, id: &str) {
    store.query_with_params("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [[$id, 'article', 'Title', [], null, null, null, null, validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}", BTreeMap::from([("id".into(), DataValue::from(id))])).unwrap();
}
fn key(kind: StateKind) -> StateKey {
    StateKey {
        kind,
        target: if kind == StateKind::Subscription {
            "https://example.com/feed"
        } else {
            "article"
        }
        .into(),
    }
}
fn sub(present: bool) -> StateValue {
    StateValue::Subscription(SubscriptionState {
        subscribed: present,
        title: Some("Café 文".into()),
        folder: Some("Design/Essays".into()),
        auto_mark_read: false,
    })
}
fn write(store: &Store, signer: &Keypair, value: StateValue, at: i64) -> StateRevision {
    store
        .write_private_state(key(value.kind()), value, at, signer)
        .unwrap()
}
fn projection(store: &Store) -> Vec<DataValue> {
    store.query("?[state, progress, last_position] := *read_state{item_id: 'article', state, progress, last_position}").unwrap().rows.remove(0)
}

#[test]
fn concurrent_intents_use_selected_policies_and_later_actions_reverse_them() {
    let p = pair();
    for (base_value, av, bv, expected, later) in [
        (
            StateValue::Read(ReadIntent::Unread),
            StateValue::Read(ReadIntent::Read),
            StateValue::Read(ReadIntent::Unread),
            StateValue::Read(ReadIntent::Unread),
            StateValue::Read(ReadIntent::Read),
        ),
        (
            StateValue::Star(false),
            StateValue::Star(true),
            StateValue::Star(false),
            StateValue::Star(true),
            StateValue::Star(false),
        ),
        (sub(true), sub(true), sub(false), sub(false), sub(true)),
    ] {
        let base = write(&p.a, &p.ka, base_value, 10);
        p.b.accept_self_state_revision(&base, p.ka.public())
            .unwrap();
        let a = write(&p.a, &p.ka, av, i64::MAX);
        let b = write(&p.b, &p.kb, bv, i64::MIN);
        p.a.accept_self_state_revision(&b, p.kb.public()).unwrap();
        p.b.accept_self_state_revision(&a, p.ka.public()).unwrap();
        assert_eq!(
            p.a.private_state_value(&a.key).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(p.b.private_state_value(&a.key).unwrap(), Some(expected));
        let query = match a.key.kind {
            StateKind::Read => {
                "?[state, progress, progress_label, last_position, updated_at] := *read_state{item_id: 'article', state, progress, progress_label, last_position, updated_at}"
            }
            StateKind::Star => {
                "?[starred, updated_at] := *item_star{item_id: 'article', starred, updated_at}"
            }
            StateKind::Subscription => "?[url, title, folder] := *feed{url, title, folder}",
            StateKind::Position => unreachable!(),
        };
        assert_eq!(
            p.a.query(query).unwrap().rows,
            p.b.query(query).unwrap().rows
        );
        assert_eq!(
            p.a.private_state_heads(&a.key).unwrap(),
            p.b.private_state_heads(&a.key).unwrap()
        );
        assert_eq!(
            p.a.accept_self_state_revision(&base, p.kb.public())
                .unwrap(),
            StateReceive::Existing
        );
        let action = write(&p.a, &p.ka, later.clone(), -100);
        assert_eq!(action.parents.len(), 2);
        p.b.accept_self_state_revision(&action, p.ka.public())
            .unwrap();
        assert_eq!(
            p.a.query(query).unwrap().rows,
            p.b.query(query).unwrap().rows
        );
        for store in [&p.a, &p.b] {
            assert_eq!(
                store.private_state_value(&action.key).unwrap(),
                Some(later.clone())
            );
            assert_eq!(
                store.private_state_heads(&action.key).unwrap(),
                vec![action.clone()]
            );
        }
    }
    assert_eq!(projection(&p.a)[0], DataValue::from("read"));
    assert_eq!(projection(&p.a)[1], DataValue::from(1.0));
    assert_eq!(
        p.a.get_feed("https://example.com/feed")
            .unwrap()
            .unwrap()
            .folder
            .as_deref(),
        Some("Design/Essays")
    );
}

#[test]
fn scroll_writes_never_override_explicit_intent_and_positions_can_move_backwards() {
    let p = pair();
    let initial = write(&p.a, &p.ka, StateValue::Read(ReadIntent::Read), 10);
    p.b.accept_self_state_revision(&initial, p.ka.public())
        .unwrap();
    p.b.record_read_progress("article", 0.9, 900.0, i64::MAX, &p.kb)
        .unwrap();
    let delayed =
        p.b.private_state_heads(&key(StateKind::Position))
            .unwrap()
            .remove(0);
    let unread = write(&p.a, &p.ka, StateValue::Read(ReadIntent::Unread), i64::MIN);
    p.a.accept_self_state_revision(&delayed, p.kb.public())
        .unwrap();
    p.b.accept_self_state_revision(&unread, p.ka.public())
        .unwrap();
    assert_eq!(projection(&p.a), projection(&p.b));
    assert_eq!(projection(&p.a)[0], DataValue::from("unread"));
    p.a.record_read_progress("article", 0.05, 10.0, i64::MIN, &p.ka)
        .unwrap();
    let position =
        p.a.private_state_heads(&key(StateKind::Position))
            .unwrap()
            .remove(0);
    p.b.accept_self_state_revision(&position, p.ka.public())
        .unwrap();
    assert_eq!(projection(&p.a), projection(&p.b));
    assert_eq!(projection(&p.a)[1], DataValue::from(0.05));
    assert_eq!(
        p.a.private_state_heads(&key(StateKind::Read)).unwrap(),
        vec![unread]
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            p.a.record_read_progress("article", bad, 0.0, 1, &p.ka)
                .is_err()
        );
        assert!(
            p.a.record_read_progress("article", 0.5, bad, 1, &p.ka)
                .is_err()
        );
    }
    p.a.record_read_progress("article", 1.5, -4.0, 0, &p.ka)
        .unwrap();
    assert_eq!(projection(&p.a)[1], DataValue::from(1.0));
    assert_eq!(projection(&p.a)[0], DataValue::from("unread"));
}

#[test]
fn implicit_reading_and_its_adoption_never_compete_with_explicit_completion() {
    let p = pair();
    p.b.record_read_progress("article", 0.2, 200.0, 1, &p.kb)
        .unwrap();
    p.b.record_read_progress("article", 0.4, 400.0, 2, &p.kb)
        .unwrap();
    assert_eq!(p.b.adopt_private_state(3, &p.kb).unwrap(), 0);
    assert!(
        p.b.private_state_heads(&key(StateKind::Read))
            .unwrap()
            .is_empty()
    );
    let completed = write(&p.a, &p.ka, StateValue::Read(ReadIntent::Read), 0);
    for record in p.b.private_state_outbox(100).unwrap() {
        p.a.accept_self_state_revision(&record, p.kb.public())
            .unwrap();
    }
    p.a.promote_pending_state().unwrap();
    p.b.accept_self_state_revision(&completed, p.ka.public())
        .unwrap();
    assert_eq!(projection(&p.a), projection(&p.b));
    assert_eq!(projection(&p.a)[0], DataValue::from("read"));
    assert_eq!(projection(&p.a)[1], DataValue::from(1.0));
}

#[test]
fn late_http_results_and_unsigned_snapshots_cannot_resurrect_removed_membership() {
    let p = pair();
    p.a.add_private_feed("https://example.com/feed", 1, &p.ka)
        .unwrap();
    p.a.remove_private_feed("https://example.com/feed", 2, &p.ka)
        .unwrap();
    let before = p.a.private_state_outbox(100).unwrap();
    let metadata = carrel_store::feeds::FeedFetchMetadata::default();
    let parsed = carrel_core::feed::ParsedFeed {
        title: Some("From RSS".into()),
        description: None,
        language: None,
        entries: vec![],
    };
    assert!(
        p.a.ingest_feed("https://example.com/feed", &parsed, &metadata)
            .is_err()
    );
    assert!(
        p.a.record_feed_not_modified("https://example.com/feed", &metadata)
            .is_err()
    );
    assert!(
        p.a.record_feed_fetch_failure("https://example.com/feed", &metadata)
            .is_err()
    );
    assert!(
        p.a.import_feed_subscriptions(&[FeedSubscription {
            url: "https://example.com/feed".into(),
            title: None,
            folder: None
        }])
        .is_err()
    );
    assert!(p.a.get_feed("https://example.com/feed").unwrap().is_none());
    assert_eq!(p.a.private_state_outbox(100).unwrap(), before);
    p.a.import_private_subscriptions(
        &[FeedSubscription {
            url: "https://example.com/feed".into(),
            title: Some("Chosen title".into()),
            folder: Some("Art".into()),
        }],
        3,
        &p.ka,
    )
    .unwrap();
    p.a.ingest_feed("https://example.com/feed", &parsed, &metadata)
        .unwrap();
    assert_eq!(
        p.a.get_feed("https://example.com/feed")
            .unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("Chosen title")
    );
    assert!(p.a.remove_feed("https://example.com/feed").is_err());
}

#[test]
fn children_and_missing_item_targets_survive_restart_and_promote_in_causal_order() {
    let p = pair();
    let root = write(&p.a, &p.ka, StateValue::Star(true), 2);
    let child = write(&p.a, &p.ka, StateValue::Star(false), 1);
    let dir = tempfile::tempdir().unwrap();
    {
        let b = Store::open(dir.path()).unwrap();
        b.migrate().unwrap();
        bind(&b, &p.master, &p.kb, &p.ka);
        assert_eq!(
            b.accept_self_state_revision(&child, p.ka.public()).unwrap(),
            StateReceive::Pending
        );
        assert_eq!(
            b.accept_self_state_revision(&root, p.ka.public()).unwrap(),
            StateReceive::Pending
        );
        assert!(b.private_state_outbox(100).unwrap().is_empty());
    }
    let b = Store::open(dir.path()).unwrap();
    b.migrate().unwrap();
    item(&b, "article");
    assert_eq!(b.promote_pending_state().unwrap().applied.len(), 2);
    assert_eq!(b.private_state_heads(&root.key).unwrap(), vec![child]);
    assert_eq!(b.private_state_outbox(100).unwrap().len(), 2);
    assert!(
        b.query("?[r] := *self_state_incoming{revision_id: r}")
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn blocked_queue_prefix_does_not_starve_ready_work_and_future_tombstones_dominate() {
    let p = pair();
    for index in 0..105 {
        let record = StateRevision::sign(
            StateKey {
                kind: StateKind::Star,
                target: format!("a-blocked-{index:03}"),
            },
            StateValue::Star(true),
            vec![format!("{index:064x}")],
            0,
            &p.ka,
        )
        .unwrap();
        assert_eq!(
            p.b.accept_self_state_revision(&record, p.ka.public())
                .unwrap(),
            StateReceive::Pending
        );
    }
    let key = StateKey {
        kind: StateKind::Star,
        target: "z-ready".into(),
    };
    let record =
        StateRevision::sign(key.clone(), StateValue::Star(true), vec![], 0, &p.ka).unwrap();
    p.b.accept_self_state_revision(&record, p.ka.public())
        .unwrap();
    item(&p.b, "z-ready");
    assert_eq!(
        p.b.promote_pending_state().unwrap().applied,
        vec![key.register()]
    );
    p.b.query("?[id, tombstoned_at, reason] <- [['a-blocked-000', validity(9223372036854775807), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    assert_eq!(p.b.promote_pending_state().unwrap().rejected.len(), 1);
    let removed = StateRevision::sign(
        StateKey {
            kind: StateKind::Star,
            target: "a-blocked-000".into(),
        },
        StateValue::Star(false),
        vec![],
        1,
        &p.ka,
    )
    .unwrap();
    assert_eq!(
        p.b.accept_self_state_revision(&removed, p.ka.public())
            .unwrap(),
        StateReceive::Removed
    );
    assert_eq!(
        p.b.query("?[count(rev)] := *self_state_incoming{revision_id: rev}")
            .unwrap()
            .rows[0][0],
        DataValue::from(104)
    );
}

#[test]
fn untrusted_forged_and_revoked_deliveries_fail_but_active_forwarders_preserve_history() {
    let p = pair();
    let record = write(&p.a, &p.ka, StateValue::Star(true), 1);
    let stranger = Keypair::from_seed(&[64; 32]);
    assert!(matches!(
        p.b.accept_self_state_revision(&record, stranger.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    let unregistered = StateRevision::sign(
        key(StateKind::Star),
        StateValue::Star(false),
        vec![],
        1,
        &stranger,
    )
    .unwrap();
    assert!(matches!(
        p.b.accept_self_state_revision(&unregistered, p.ka.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    let mut forged = record.clone();
    forged.value = StateValue::Star(false);
    assert!(
        p.b.accept_self_state_revision(&forged, p.ka.public())
            .is_err()
    );
    p.b.accept_self_revocation(&RevocationCert::new_at_micros(&p.master, p.ka.public(), 2))
        .unwrap();
    assert!(matches!(
        p.b.accept_self_state_revision(&record, p.ka.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(
        p.b.accept_self_state_revision(&record, p.kb.public())
            .unwrap(),
        StateReceive::Applied
    );
    p.a.accept_self_revocation(&RevocationCert::new_at_micros(&p.master, p.ka.public(), 2))
        .unwrap();
    assert!(matches!(
        p.a.toggle_private_star("article", 3, &p.ka),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(p.a.private_state_heads(&record.key).unwrap(), vec![record]);
    let unbound = Store::open_in_memory().unwrap();
    unbound.migrate().unwrap();
    assert!(matches!(
        unbound.accept_self_state_revision(&unregistered, stranger.public()),
        Err(StoreError::IdentityUnbound)
    ));
}

#[test]
fn journal_projection_and_outbox_roll_back_together_on_storage_failure() {
    let p = pair();
    p.a.query("::set_triggers item_star on put { ?[id] := _new[id, starred, at] :assert none }")
        .unwrap();
    assert!(p.a.toggle_private_star("article", 1, &p.ka).is_err());
    assert!(
        p.a.private_state_heads(&key(StateKind::Star))
            .unwrap()
            .is_empty()
    );
    assert!(p.a.private_state_outbox(100).unwrap().is_empty());
    assert!(
        p.a.query("?[id] := *item_star{item_id: id}")
            .unwrap()
            .rows
            .is_empty()
    );
    p.a.query("::set_triggers item_star").unwrap();
    assert!(p.a.toggle_private_star("article", 1, &p.ka).unwrap());
}

#[test]
fn local_adoption_preserves_negative_state_generic_cursors_and_http_cache() {
    let p = pair();
    p.a.query("?[item_id, state, progress, progress_label, last_position, updated_at] <- [['article', 'unread', 0.25, 'page 25', parse_json('{\"page\":25,\"cfi\":\"文\"}'), validity(9223372036854775807)]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at}").unwrap();
    p.a.query("?[item_id, starred, updated_at] <- [['article', false, validity(1)]] :put item_star {item_id => starred, updated_at}").unwrap();
    p.a.import_feed_subscriptions(&[FeedSubscription {
        url: "https://example.com/feed".into(),
        title: Some("Custom".into()),
        folder: Some("Art".into()),
    }])
    .unwrap();
    p.a.query("?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] <- [['https://example.com/feed', 'Custom', 'Description', validity(10), 'modified-private', 'etag-private', 7200, 4, 'Art', false]] :put feed {url => title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}").unwrap();
    let before = projection(&p.a);
    let feed = p.a.get_feed("https://example.com/feed").unwrap();
    assert_eq!(p.a.adopt_private_state(0, &p.ka).unwrap(), 4);
    assert_eq!(projection(&p.a), before);
    assert_eq!(p.a.get_feed("https://example.com/feed").unwrap(), feed);
    let records = p.a.private_state_outbox(100).unwrap();
    assert_eq!(records.len(), 4);
    for record in &records {
        let wire = serde_json::to_string(record).unwrap();
        assert!(!wire.contains("etag-private") && !wire.contains("modified-private"));
        p.b.accept_self_state_revision(record, p.ka.public())
            .unwrap();
    }
    assert_eq!(projection(&p.b), before);
    assert_eq!(
        p.b.get_feed("https://example.com/feed")
            .unwrap()
            .unwrap()
            .etag_header,
        None
    );
    let action = write(&p.a, &p.ka, StateValue::Star(true), -10);
    assert_eq!(p.a.adopt_private_state(20, &p.ka).unwrap(), 0);
    assert_eq!(p.a.private_state_heads(&action.key).unwrap(), vec![action]);
}

#[test]
fn subscription_import_is_atomic_preserves_settings_and_resubscription_is_causal() {
    let p = pair();
    let good = FeedSubscription {
        url: "https://example.com/feed".into(),
        title: Some("Initial".into()),
        folder: Some("Design".into()),
    };
    let bad = FeedSubscription {
        url: "https://example.com/z".into(),
        title: Some("x".repeat(4097)),
        folder: None,
    };
    assert!(
        p.a.import_private_subscriptions(&[good.clone(), bad], 1, &p.ka)
            .is_err()
    );
    assert!(p.a.list_feeds().unwrap().is_empty());
    assert!(p.a.private_state_outbox(100).unwrap().is_empty());
    p.a.import_private_subscriptions(std::slice::from_ref(&good), 1, &p.ka)
        .unwrap();
    let first =
        p.a.private_state_heads(&key(StateKind::Subscription))
            .unwrap()
            .remove(0);
    p.a.import_private_subscriptions(
        &[FeedSubscription {
            title: Some("Replacement".into()),
            folder: None,
            ..good.clone()
        }],
        2,
        &p.ka,
    )
    .unwrap();
    assert_eq!(p.a.get_feed(&good.url).unwrap().unwrap().title, good.title);
    assert_eq!(
        p.a.private_state_heads(&first.key).unwrap(),
        vec![first.clone()]
    );
    assert!(p.a.remove_private_feed(&good.url, 3, &p.ka).unwrap());
    let removed = p.a.private_state_heads(&first.key).unwrap().remove(0);
    assert_eq!(removed.parents, vec![first.revision_id.clone()]);
    p.a.accept_self_state_revision(&first, p.kb.public())
        .unwrap();
    assert!(p.a.get_feed(&good.url).unwrap().is_none());
    p.a.add_private_feed(&good.url, 0, &p.ka).unwrap();
    let again = p.a.private_state_heads(&first.key).unwrap().remove(0);
    assert_eq!(again.parents, vec![removed.revision_id]);
    p.a.acknowledge_private_state(&first.key, &first.revision_id)
        .unwrap();
    assert!(p.a.private_state_outbox(100).unwrap().contains(&again));
    assert_eq!(p.a.private_state_outbox(1).unwrap().len(), 1);
    assert!(p.a.private_state_outbox(0).is_err());
    assert!(p.a.private_state_outbox(1001).is_err());
}

#[test]
fn a_first_signed_intent_adopts_legacy_cursor_before_changing_completion() {
    let p = pair();
    p.a.query("?[item_id, state, progress, progress_label, last_position, updated_at] <- [['article', 'reading', 0.25, '25%', parse_json('{\"page\":25}'), validity(9223372036854775807)]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at}").unwrap();
    write(&p.a, &p.ka, StateValue::Read(ReadIntent::Read), 10);
    write(&p.a, &p.ka, StateValue::Read(ReadIntent::Unread), 5);
    assert_eq!(projection(&p.a)[1], DataValue::from(0.25));
    for record in p.a.private_state_outbox(100).unwrap() {
        p.b.accept_self_state_revision(&record, p.ka.public())
            .unwrap();
    }
    p.b.promote_pending_state().unwrap();
    let query = "?[state, progress, progress_label, last_position, updated_at] := *read_state{item_id: 'article', state, progress, progress_label, last_position, updated_at}";
    assert_eq!(
        p.a.query(query).unwrap().rows,
        p.b.query(query).unwrap().rows
    );
}

#[test]
fn rejected_legacy_adoption_keeps_the_original_cursor_and_creates_no_intent() {
    let p = pair();
    let cursor = serde_json::json!({"cfi": "x".repeat(16*1024)});
    p.a.query_with_params("?[item_id, state, progress, progress_label, last_position, updated_at] <- [['article', 'unread', 0.25, '25%', $cursor, validity(1)]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at}", BTreeMap::from([("cursor".into(), DataValue::Json(cozo::JsonData(cursor)))] )).unwrap();
    let before = projection(&p.a);
    assert!(p.a.adopt_private_state(10, &p.ka).is_err());
    assert_eq!(projection(&p.a), before);
    assert!(p.a.private_state_outbox(100).unwrap().is_empty());
    assert!(
        p.a.private_state_heads(&key(StateKind::Read))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn clone_writes_serialize_and_raw_history_cannot_be_changed_or_erased() {
    let p = pair();
    std::thread::scope(|scope| {
        let first = p.a.clone();
        let second = p.a.clone();
        let signer = &p.ka;
        scope.spawn(move || first.toggle_private_star("article", 1, signer).unwrap());
        scope.spawn(move || second.toggle_private_star("article", 1, signer).unwrap());
    });
    assert_eq!(
        p.a.private_state_value(&key(StateKind::Star)).unwrap(),
        Some(StateValue::Star(false))
    );
    let record =
        p.a.private_state_heads(&key(StateKind::Star))
            .unwrap()
            .remove(0);
    assert_eq!(record.parents.len(), 1);
    let different = StateRevision::sign(
        record.key.clone(),
        StateValue::Star(true),
        record.parents.clone(),
        2,
        &p.ka,
    )
    .unwrap();
    for (column, replacement) in [
        ("encoded", DataValue::Bytes(different.to_bytes().unwrap())),
        ("parents", DataValue::List(vec![])),
        ("kind", DataValue::from("read")),
        ("target", DataValue::from("another")),
    ] {
        let script = "?[register, revision_id, encoded, parents, kind, target] := *self_state{register, revision_id, encoded: old_encoded, parents: old_parents, kind: old_kind, target: old_target}, revision_id = $rev, encoded = old_encoded, parents = old_parents, kind = old_kind, target = old_target :put self_state {register, revision_id => encoded, parents, kind, target}";
        let script = script.replace(
            &format!("{column} = old_{column}"),
            &format!("{column} = $replacement"),
        );
        let result = p.a.query_with_params(
            &script,
            BTreeMap::from([
                ("rev".into(), DataValue::from(record.revision_id.as_str())),
                ("replacement".into(), replacement),
            ]),
        );
        assert!(result.is_err(), "mutation of {column} must fail");
    }
    assert!(p.a.query("?[register, revision_id] := *self_state{register, revision_id} :rm self_state {register, revision_id}").is_err());
    assert_eq!(p.a.private_state_heads(&record.key).unwrap(), vec![record]);
}

#[test]
fn cross_register_ancestry_is_rejected_after_out_of_order_dependency_delivery() {
    let p = pair();
    let root = write(&p.a, &p.ka, StateValue::Read(ReadIntent::Read), 1);
    let malformed = StateRevision::sign(
        key(StateKind::Star),
        StateValue::Star(true),
        vec![root.revision_id.clone()],
        2,
        &p.ka,
    )
    .unwrap();
    assert_eq!(
        p.b.accept_self_state_revision(&malformed, p.ka.public())
            .unwrap(),
        StateReceive::Pending
    );
    p.b.accept_self_state_revision(&root, p.ka.public())
        .unwrap();
    assert_eq!(
        p.b.promote_pending_state().unwrap().rejected,
        vec![malformed.revision_id.clone()]
    );
    assert!(
        p.b.accept_self_state_revision(&malformed, p.ka.public())
            .is_err()
    );
    assert!(
        p.b.private_state_heads(&key(StateKind::Star))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn all_delivery_orders_and_duplicate_retries_converge_with_offline_branches() {
    let original = pair();
    let root = write(&original.a, &original.ka, StateValue::Star(false), 100);
    original
        .b
        .accept_self_state_revision(&root, original.ka.public())
        .unwrap();
    let a = write(&original.a, &original.ka, StateValue::Star(true), i64::MAX);
    let b = write(&original.b, &original.kb, StateValue::Star(false), i64::MIN);
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let receiver = pair();
        let records = [&root, &a, &b];
        for index in order {
            for _ in 0..2 {
                receiver
                    .b
                    .accept_self_state_revision(records[index], original.ka.public())
                    .unwrap();
            }
            receiver.b.promote_pending_state().unwrap();
        }
        assert_eq!(
            receiver.b.private_state_value(&root.key).unwrap(),
            Some(StateValue::Star(true))
        );
        assert_eq!(receiver.b.private_state_heads(&root.key).unwrap().len(), 2);
        assert_eq!(receiver.b.private_state_outbox(100).unwrap().len(), 3);
    }
}
