use carrel_store::{CURRENT_SCHEMA_VERSION, Store, schema};
use cozo::DataValue;
use std::collections::BTreeMap;

#[test]
fn migration_creates_relations_and_is_idempotent() {
    let store = Store::open_in_memory().unwrap();

    assert_eq!(store.current_schema_version().unwrap(), 0);

    store.migrate().unwrap();
    assert_eq!(
        store.current_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );

    let relations = store.query("::relations").unwrap();
    for expected in schema::RELATIONS {
        assert!(
            relations
                .rows
                .iter()
                .any(|row| matches!(row.first(), Some(DataValue::Str(name)) if name == expected)),
            "missing relation {expected}"
        );
    }

    store.migrate().unwrap();
    assert_eq!(
        store.current_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn persistent_store_open_reopens_migrated_schema() {
    let tempdir = tempfile::tempdir().unwrap();
    let store_path = tempdir.path().join("store");

    {
        let store = Store::open(&store_path).unwrap();
        store.migrate().unwrap();
        assert_eq!(
            store.current_schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    let reopened = Store::open(&store_path).unwrap();
    assert_eq!(
        reopened.current_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn search_migration_preserves_v1_facts_and_resumes_partial_creation() {
    for stage in 0..=2 {
        let store = Store::open_in_memory().unwrap();
        store.query(schema::CREATE_SCHEMA).unwrap();
        store.query("?[version, applied_at, description] <- [[1, validity(1), 'initial schema']] :put schema_version {version => applied_at, description}").unwrap();
        store.query(r#"
            ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [['old', 'article', 'Preserved café title', [], null, null, null, null, validity(1)]]
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
        "#).unwrap();
        store.query("?[item_id, starred, updated_at] <- [['old', true, validity(1)]] :put item_star {item_id => starred, updated_at}").unwrap();
        if stage >= 1 {
            store.query(schema::CREATE_SEARCH_SCHEMA).unwrap();
        }
        if stage >= 2 {
            store.query(schema::CREATE_TITLE_SEARCH_INDEX).unwrap();
        }
        store.migrate().unwrap();
        let page = store
            .list_item_page(
                &carrel_core::library::ItemFilter {
                    search: Some("CAF".into()),
                    ..carrel_core::library::ItemFilter::starred()
                },
                0,
            )
            .unwrap();
        assert_eq!(page.items[0].id, "old");
        assert!(page.items[0].starred);
        store.migrate().unwrap();
        assert_eq!(
            store.current_schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
}

#[test]
fn note_history_migration_preserves_legacy_signatures_and_resumes_each_stage() {
    use carrel_core::identity::{Keypair, PublicKey, sign_canonical};
    use carrel_store::highlights::EditNoteInput;
    use cozo::Validity;
    use serde::Serialize;

    #[derive(Serialize)]
    struct LegacyNote<'a> {
        relation: &'static str,
        id: &'a str,
        body: &'a str,
        target_kind: Option<&'a str>,
        target_id: Option<&'a str>,
        created_by: PublicKey,
        created_at_micros: i64,
        updated_at_micros: i64,
    }
    let key = Keypair::from_seed(&[23; 32]);
    let legacy = LegacyNote {
        relation: "note",
        id: "stable-legacy",
        body: "Legacy café body",
        target_kind: Some("item"),
        target_id: Some("old-item"),
        created_by: *key.public(),
        created_at_micros: 1_700_000_000_000_000,
        updated_at_micros: 1_700_000_000_000_005,
    };
    let signature = sign_canonical(&key, &legacy).unwrap();
    for stage in 0..=4 {
        let store = Store::open_in_memory().unwrap();
        store.query(schema::CREATE_SCHEMA).unwrap();
        store.query("?[version, applied_at, description] <- [[2, validity(1), 'local search']] :put schema_version {version => applied_at, description}").unwrap();
        store.query(r#"
            ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [['old-item', 'article', 'Old item', [], null, null, null, null, validity(1)]]
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
        "#).unwrap();
        store.query_with_params(r#"
            ?[id, body, target_kind, target_id, created_by, created_at, updated_at, signature] <-
                [['stable-legacy', 'Legacy café body', 'item', 'old-item', $key, $created, $updated, $signature]]
            :put note {id => body, target_kind, target_id, created_by, created_at, updated_at, signature}
        "#, BTreeMap::from([
            ("key".into(), DataValue::Bytes(key.public().as_bytes().to_vec())),
            ("created".into(), DataValue::Validity(Validity::from((legacy.created_at_micros, true)))),
            ("updated".into(), DataValue::Validity(Validity::from((legacy.updated_at_micros, true)))),
            ("signature".into(), DataValue::Bytes(signature.as_bytes().to_vec())),
        ])).unwrap();
        if stage >= 1 {
            store.query(schema::CREATE_NOTE_REVISIONS).unwrap();
        }
        if stage >= 2 {
            store.query(r#"
            ?[id, updated_at, body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature] :=
                *note{id, body, target_kind, target_id, created_by, created_at, updated_at, signature},
                updated_by = created_by, signature_version = 1
            :put note_v3 {id, updated_at => body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature}
        "#).unwrap();
        }
        if stage >= 3 {
            store
                .query("::rename note -> note_legacy_v2, note_v3 -> note")
                .unwrap();
        }
        if stage >= 4 {
            store.query(schema::CREATE_ANNOTATION_REMOVALS).unwrap();
        }
        store.migrate().unwrap();
        let preserved = store.get_note(legacy.id).unwrap().unwrap();
        assert_eq!(preserved.signature, signature);
        assert_eq!(preserved.signature_version, 1);
        assert_eq!(preserved.id, legacy.id);
        assert_eq!(preserved.body, legacy.body);
        preserved.verify_signature().unwrap();
        let updated = store
            .edit_note(
                EditNoteInput {
                    id: legacy.id.into(),
                    body: "A new signed revision".into(),
                    expected_revision_id: preserved.revision_id.clone(),
                    now_micros: Some(legacy.updated_at_micros + 10),
                },
                &key,
            )
            .unwrap();
        updated.verify_signature().unwrap();
        assert_eq!(updated.id, legacy.id);
        assert_eq!(updated.signature_version, 3);
        store.migrate().unwrap();
        assert_eq!(
            store
                .query("?[id, updated_at] := *note{id, updated_at}")
                .unwrap()
                .rows
                .len(),
            2
        );
    }
}
