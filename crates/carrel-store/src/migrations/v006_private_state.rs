//! Add private causal history without inventing signatures for legacy rows.

use std::collections::BTreeMap;

use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::{Result, schema};

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    for (name, script) in schema::PRIVATE_STATE_RELATIONS {
        if !run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(*name)))
        {
            run(script)?;
        }
    }
    for script in schema::PRIVATE_STATE_GUARDS {
        run(script)?;
    }
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[6, $now, 'signed private causal state and durable mirror queue']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn every_interrupted_stage_preserves_unsigned_state_and_resumes_without_signing() {
        for stage in 0..=6 {
            let store = Store::open_in_memory().unwrap();
            crate::migrations::v001_initial::up(&store.db).unwrap();
            crate::migrations::v002_search::up(&store.db).unwrap();
            crate::migrations::v003_annotations::up(&store.db).unwrap();
            crate::migrations::v004_identity::up(&store.db).unwrap();
            crate::migrations::v005_causal_notes::up(&store.db).unwrap();
            store.query("?[item_id, state, progress, progress_label, last_position, updated_at] <- [['old', 'unread', 0.5, 'Halfway', parse_json('{\"page\":4}'), validity(9223372036854775807)]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at}").unwrap();
            store.query("?[item_id, starred, updated_at] <- [['old', false, validity(1)]] :put item_star {item_id => starred, updated_at}").unwrap();
            let before = store.query("?[item_id, state, progress, progress_label, last_position, updated_at] := *read_state{item_id, state, progress, progress_label, last_position, updated_at}").unwrap();
            for (_, script) in schema::PRIVATE_STATE_RELATIONS.iter().take(stage) {
                store.query(script).unwrap();
            }
            if stage >= 4 {
                store.query(schema::PRIVATE_STATE_GUARDS[0]).unwrap();
            }
            if stage >= 5 {
                store.query(schema::PRIVATE_STATE_GUARDS[1]).unwrap();
            }
            if stage >= 6 {
                store.query(schema::PRIVATE_STATE_GUARDS[2]).unwrap();
            }
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                crate::CURRENT_SCHEMA_VERSION
            );
            assert_eq!(store.query("?[item_id, state, progress, progress_label, last_position, updated_at] := *read_state{item_id, state, progress, progress_label, last_position, updated_at}").unwrap().rows, before.rows);
            assert_eq!(
                store
                    .query("?[starred] := *item_star{starred}")
                    .unwrap()
                    .rows,
                vec![vec![DataValue::Bool(false)]]
            );
            assert!(store.private_state_outbox(100).unwrap().is_empty());
            assert!(
                store
                    .query("?[reg] := *self_state{register: reg}")
                    .unwrap()
                    .rows
                    .is_empty()
            );
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                crate::CURRENT_SCHEMA_VERSION
            );
        }
    }
}
