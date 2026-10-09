use carrel_store::{CURRENT_SCHEMA_VERSION, Store, schema};
use cozo::DataValue;

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
        assert_eq!(store.current_schema_version().unwrap(), 2);
    }
}
