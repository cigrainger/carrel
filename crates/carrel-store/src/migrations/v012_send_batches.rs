//! Add a local atomic-send retry manifest without changing existing signed facts.
use crate::Result;
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let relations = db.run_script("::relations", BTreeMap::new(), ScriptMutability::Mutable)?;
    if !relations
        .rows
        .iter()
        .any(|row| row.first() == Some(&DataValue::from("social_send")))
    {
        db.run_script(
            ":create social_send {id:String=>request:Bytes,shares:[String]}",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        )?;
    }
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script("?[version,applied_at,description] <- [[12,$now,'atomic explicit audience send batches']] :put schema_version {version=>applied_at,description}",BTreeMap::from([("now".into(),DataValue::Validity(Validity::from((now,true))))]),ScriptMutability::Mutable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{CURRENT_SCHEMA_VERSION, Store};
    #[test]
    fn interrupted_send_migration_resumes_without_selecting_an_identity() {
        for created in [false, true] {
            let store = Store::open_in_memory().unwrap();
            store.migrate().unwrap();
            store
                .query("?[version] <- [[12]] :rm schema_version {version}")
                .unwrap();
            if !created {
                store.query("::remove social_send").unwrap();
            }
            store.migrate().unwrap();
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                CURRENT_SCHEMA_VERSION
            );
            assert!(store.self_identity().unwrap().is_none());
            assert!(
                store
                    .query("?[id] := *social_send{id}")
                    .unwrap()
                    .rows
                    .is_empty()
            );
        }
    }
}
