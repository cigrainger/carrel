//! Add typed social records without changing existing private archives or identity.
use crate::{Result, schema};
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    for (name, script) in schema::SOCIAL_RELATIONS {
        if !run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(*name)))
        {
            run(script)?;
        }
    }
    for script in schema::SOCIAL_GUARDS {
        run(script)?;
    }
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script("?[version, applied_at, description] <- [[9, $now, 'signed recipient-scoped shares, contacts, receipts and social mirror binding']] :put schema_version {version => applied_at, description}", BTreeMap::from([("now".into(),DataValue::Validity(Validity::from((now,true))))]),ScriptMutability::Mutable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use proptest::prelude::*;

    fn version_eight() -> Store {
        let store = Store::open_in_memory().unwrap();
        super::super::v001_initial::up(&store.db).unwrap();
        super::super::v002_search::up(&store.db).unwrap();
        super::super::v003_annotations::up(&store.db).unwrap();
        super::super::v004_identity::up(&store.db).unwrap();
        super::super::v005_causal_notes::up(&store.db).unwrap();
        super::super::v006_private_state::up(&store.db).unwrap();
        super::super::v007_portable_legacy_notes::up(&store.db).unwrap();
        super::super::v008_private_library::up(&store.db).unwrap();
        store.query("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [['older', 'article', 'Café 文', [], null, null, null, null, validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}").unwrap();
        store
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        #[test]
        fn interrupted_additive_sharing_migration_preserves_archives_and_never_follows_anyone(prefix in 0usize..=schema::SOCIAL_RELATIONS.len()) {
            let store=version_eight();
            let before=store.query("?[id, title] := *item{id, title}").unwrap().rows;
            for (_,script) in schema::SOCIAL_RELATIONS.iter().take(prefix) {store.query(script).unwrap();}
            prop_assert_eq!(store.current_schema_version().unwrap(),8);
            store.migrate().unwrap();
            prop_assert_eq!(store.current_schema_version().unwrap(),crate::CURRENT_SCHEMA_VERSION);
            prop_assert_eq!(store.query("?[id, title] := *item{id, title}").unwrap().rows,before);
            prop_assert!(store.self_identity().unwrap().is_none());
            prop_assert!(store.query("?[master] := *social_contact{master}").unwrap().rows.is_empty(), "no contacts");
            prop_assert!(store.query("?[key] := *social_record{key}").unwrap().rows.is_empty(), "no shares");
            prop_assert!(store.query("?[n] := *social_document{namespace:n}").unwrap().rows.is_empty(), "no mirror binding");
            up(&store.db).unwrap();
            prop_assert_eq!(store.current_schema_version().unwrap(),crate::CURRENT_SCHEMA_VERSION);
        }
    }
}
