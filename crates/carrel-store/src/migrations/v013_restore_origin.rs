//! Local recovery provenance never authorizes a network peer.
use crate::Result;
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let rows = db.run_script("::relations", BTreeMap::new(), ScriptMutability::Mutable)?;
    if !rows
        .rows
        .iter()
        .any(|row| row.first() == Some(&DataValue::from("restore_origin")))
    {
        db.run_script(":create restore_origin {singleton:Int=>master_pubkey:Bytes?,device_pubkey:Bytes,legacy_devices:[Bytes]}", BTreeMap::new(), ScriptMutability::Mutable)?;
    }
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script("?[version,applied_at,description] <- [[13,$now,'local backup recovery provenance']] :put schema_version {version=>applied_at,description}", BTreeMap::from([("now".into(),DataValue::Validity(Validity::from((now,true))))]), ScriptMutability::Mutable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{CURRENT_SCHEMA_VERSION, Store};
    #[test]
    fn interrupted_recovery_migration_resumes_without_choosing_an_identity() {
        for exists in [false, true] {
            let store = Store::open_in_memory().unwrap();
            super::super::migrate_through(&store.db, 0, 12).unwrap();
            if exists {
                store.query(":create restore_origin {singleton:Int=>master_pubkey:Bytes?,device_pubkey:Bytes,legacy_devices:[Bytes]}").unwrap();
            }
            store.migrate().unwrap();
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                CURRENT_SCHEMA_VERSION
            );
            assert!(store.self_identity().unwrap().is_none());
            assert!(store.restore_origin().unwrap().is_none());
        }
    }
}
