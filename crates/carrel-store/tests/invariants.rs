use std::collections::BTreeMap;

use carrel_store::Store;
use carrel_store::ids::{canonicalize_external_identifier, id_for_authored, id_for_external};
use cozo::{DataValue, Num, Validity};
use proptest::prelude::*;

proptest! {
    #[test]
    fn external_ids_are_stable(input in "\\PC*") {
        let first = id_for_external(&input);
        let second = id_for_external(&input);

        prop_assert_eq!(first.len(), 64);
        prop_assert_eq!(first, second);
    }

    #[test]
    fn authored_ids_are_stable(content in prop::collection::vec(any::<u8>(), 0..256), author in any::<[u8; 32]>(), created_at_secs in any::<i64>()) {
        let first = id_for_authored(&content, &author, created_at_secs);
        let second = id_for_authored(&content, &author, created_at_secs);

        prop_assert_eq!(first.len(), 64);
        prop_assert_eq!(first, second);
    }
}

#[test]
fn url_canonicalization_strips_tracking_parameters() {
    let canonical = canonicalize_external_identifier(
        " HTTPS://Example.COM/path/?utm_source=newsletter&b=2&fbclid=nope&a=1&ref=feed ",
    );

    assert_eq!(canonical, "https://example.com/path?a=1&b=2");
}

#[test]
fn read_state_rejects_older_validity_for_same_item() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();

    put_read_state(&store, "item-1", "reading", 20).unwrap();
    let err = put_read_state(&store, "item-1", "unread", 10).unwrap_err();
    assert!(!err.to_string().is_empty());

    let rows = store
        .query("?[state, updated_at] := *read_state{item_id: 'item-1', state, updated_at}")
        .unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0][0], DataValue::from("reading"));
    assert_eq!(
        rows.rows[0][1],
        DataValue::Validity(Validity::from((20, true)))
    );
}

#[test]
fn audience_kind_cannot_change() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();

    store
        .query(
            r#"
            ?[id, name, kind, created_at] :=
                id = 'audience-1',
                name = 'Friends',
                kind = 'private',
                created_at = 'ASSERT'
            :put audience {id => name, kind, created_at}
            "#,
        )
        .unwrap();

    let err = store
        .query(
            r#"
            ?[id, name, kind, created_at] :=
                id = 'audience-1',
                name = 'Friends',
                kind = 'public',
                created_at = 'ASSERT'
            :put audience {id => name, kind, created_at}
            "#,
        )
        .unwrap_err();

    assert!(!err.to_string().is_empty());
}

#[test]
fn tombstoned_item_id_cannot_be_recreated() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();

    store
        .query_with_params(
            r#"
            ?[id, tombstoned_at, reason] :=
                id = 'item-dead',
                tombstoned_at = $tombstoned_at,
                reason = 'removed'
            :put entity_tombstone {id, tombstoned_at => reason}
            "#,
            BTreeMap::from([(
                "tombstoned_at".to_string(),
                DataValue::Validity(Validity::from((1, true))),
            )]),
        )
        .unwrap();

    let err = store.query(
            r#"
            ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                id = 'item-dead',
                kind = 'article',
                title = 'Should stay gone',
                creators = ['Ada'],
                primary_url = 'https://example.com/dead',
                published_at = null,
                language = 'en',
                summary = null,
                discovered_at = 'ASSERT'
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
            "#,
        ).unwrap_err();

    assert!(!err.to_string().is_empty());
}

fn put_read_state(
    store: &Store,
    item_id: &str,
    state: &str,
    timestamp: i64,
) -> carrel_store::Result<()> {
    store.query_with_params(
        r#"
        ?[item_id, state, progress, progress_label, last_position, updated_at] :=
            item_id = $item_id,
            state = $state,
            progress = null,
            progress_label = null,
            last_position = null,
            updated_at = $updated_at
        :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
        "#,
        BTreeMap::from([
            ("item_id".to_string(), DataValue::from(item_id.to_string())),
            ("state".to_string(), DataValue::from(state.to_string())),
            (
                "updated_at".to_string(),
                DataValue::Validity(Validity::from((timestamp, true))),
            ),
        ]),
    )?;

    Ok(())
}

#[test]
fn validity_timestamp_shape_is_int_and_assert_flag() {
    let value = DataValue::Validity(Validity::from((42, true)));

    match value {
        DataValue::Validity(validity) => {
            assert_eq!(validity.timestamp.0.0, 42);
            assert!(validity.is_assert.0);
        }
        DataValue::Num(Num::Int(_)) => unreachable!("validity must not be a plain int"),
        _ => unreachable!("unexpected validity representation"),
    }
}
