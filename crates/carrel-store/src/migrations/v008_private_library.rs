//! Preserve unsigned source metadata for explicit, resumable device adoption.

use crate::{Result, schema};
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;
use time::OffsetDateTime;

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    for (name, script) in schema::PRIVATE_LIBRARY_RELATIONS {
        if !run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(*name)))
        {
            run(script)?;
        }
    }
    for script in schema::PRIVATE_LIBRARY_GUARDS
        .iter()
        .chain(schema::PRIVATE_ITEM_DIRTY_TRIGGERS)
    {
        run(script)?;
    }
    run(r#"removed[id] := *entity_tombstone{id @ "END"}
        ?[id, token] := *item{id}, not removed[id], not *self_item_dirty{id},
            not *self_item_revision{id}, token = to_string(rand_uuid_v4())
        :put self_item_dirty {id => token}"#)?;
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[8, $now, 'private item history, fact mirror queue, and resumable source adoption']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use proptest::prelude::*;

    fn version_seven() -> Store {
        let store = Store::open_in_memory().unwrap();
        crate::migrations::v001_initial::up(&store.db).unwrap();
        crate::migrations::v002_search::up(&store.db).unwrap();
        crate::migrations::v003_annotations::up(&store.db).unwrap();
        crate::migrations::v004_identity::up(&store.db).unwrap();
        crate::migrations::v005_causal_notes::up(&store.db).unwrap();
        crate::migrations::v006_private_state::up(&store.db).unwrap();
        crate::migrations::v007_portable_legacy_notes::up(&store.db).unwrap();
        store.query("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [['older', 'article', 'Café 文', [], null, null, null, null, validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}").unwrap();
        store
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        #[test]
        fn interrupted_relation_creation_resumes_without_selecting_an_identity(prefix in 0usize..=schema::PRIVATE_LIBRARY_RELATIONS.len()) {
            let store = version_seven();
            for (_, script) in schema::PRIVATE_LIBRARY_RELATIONS.iter().take(prefix) { store.query(script).unwrap(); }
            prop_assert_eq!(store.current_schema_version().unwrap(), 7);
            store.migrate().unwrap();
            prop_assert_eq!(store.current_schema_version().unwrap(), crate::CURRENT_SCHEMA_VERSION);
            prop_assert!(store.self_identity().unwrap().is_none());
            prop_assert!(store.private_document().unwrap().is_none());
            prop_assert!(store.private_fact_outbox(None, 100).unwrap().is_empty());
            let token = store.capture_private_item("older").unwrap().unwrap().token;
            up(&store.db).unwrap();
            prop_assert_eq!(store.capture_private_item("older").unwrap().unwrap().token, token);
        }
    }
}
