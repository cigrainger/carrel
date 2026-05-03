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
