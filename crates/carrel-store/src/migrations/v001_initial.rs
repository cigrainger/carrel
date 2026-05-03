//! Initial schema migration.

use std::collections::BTreeMap;

use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::error::Result;
use crate::schema;

pub fn up(db: &DbInstance) -> Result<()> {
    db.run_script(
        schema::CREATE_SCHEMA,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )?;
    for trigger_script in [
        schema::CREATE_AUDIENCE_TRIGGERS,
        schema::CREATE_READ_STATE_TRIGGERS,
        schema::CREATE_ITEM_TRIGGERS,
    ] {
        db.run_script(trigger_script, BTreeMap::new(), ScriptMutability::Mutable)?;
    }

    let now = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000;
    db.run_script(
        r#"
        ?[version, applied_at, description] :=
            version = 1,
            applied_at = $applied_at,
            description = 'initial schema'
        :put schema_version {version => applied_at, description}
        "#,
        BTreeMap::from([(
            "applied_at".to_string(),
            DataValue::Validity(Validity::from((now as i64, true))),
        )]),
        ScriptMutability::Mutable,
    )?;

    Ok(())
}
