//! Local working text is never adopted into the signed private journal.
use crate::{Result, schema};
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let rows = db.run_script("::relations", BTreeMap::new(), ScriptMutability::Mutable)?;
    let exists = rows
        .rows
        .iter()
        .any(|row| row.first() == Some(&DataValue::from("social_reply_draft")));
    let create = if exists {
        String::new()
    } else {
        format!("{{ {} }}", schema::REPLY_DRAFT_RELATION)
    };
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script(
        &format!(
            "{create}\n{{ {} }}\n{{ ?[version,applied_at,description] <- [[15,$now,'durable installation-local reply drafts']] :put schema_version {{version=>applied_at,description}} }}",
            schema::REPLY_DRAFT_GUARD,
        ),
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{CURRENT_SCHEMA_VERSION, Store};

    #[test]
    fn empty_or_interrupted_draft_migration_is_resumable_and_keeps_14_catalog_exact() {
        for created in [false, true] {
            let store = Store::open_in_memory().unwrap();
            super::super::migrate_through(&store.db, 0, 14).unwrap();
            let guards = store.query("::show_triggers self_state").unwrap().rows;
            if created {
                store.query(crate::schema::REPLY_DRAFT_RELATION).unwrap();
            }
            store.migrate().unwrap();
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                store.query("::show_triggers self_state").unwrap().rows,
                guards
            );
            assert!(store.is_empty_for_restore().unwrap());
            assert!(store.self_identity().unwrap().is_none());
        }
    }

    #[test]
    fn version_write_failure_rolls_back_the_relation_and_guards() {
        let store = Store::open_in_memory().unwrap();
        super::super::migrate_through(&store.db, 0, 14).unwrap();
        let old_guards = store.query("::show_triggers self_state").unwrap().rows;
        store.query("::set_triggers schema_version on put { ?[version] := _new[version,at,description], version == 15 :assert none }").unwrap();
        assert!(super::up(&store.db).is_err());
        assert_eq!(store.current_schema_version().unwrap(), 14);
        assert!(!store.relation_exists("social_reply_draft").unwrap());
        assert_eq!(
            store.query("::show_triggers self_state").unwrap().rows,
            old_guards
        );
        store.query("::set_triggers schema_version").unwrap();
        store.migrate().unwrap();
        assert_eq!(
            store.current_schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
}
