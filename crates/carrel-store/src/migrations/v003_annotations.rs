//! Preserve legacy signatures while introducing immutable note revisions.

use std::collections::BTreeMap;

use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::{Result, schema};

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    let exists = |name: &str| -> Result<bool> {
        Ok(run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(name))))
    };
    let revised = exists("note")?
        && run("::columns note")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from("updated_by")));
    if !revised {
        if !exists("note_v3")? {
            run(schema::CREATE_NOTE_REVISIONS)?;
        }
        let source = if exists("note_legacy_v2")? {
            "note_legacy_v2"
        } else {
            "note"
        };
        run(&format!(
            r#"
            ?[id, updated_at, body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature] :=
                *{source}{{id, body, target_kind, target_id, created_by, created_at, updated_at, signature}},
                updated_by = created_by, signature_version = 1
            :put note_v3 {{id, updated_at => body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature}}
        "#
        ))?;
        // A single system transaction makes both names change together.
        if exists("note")? {
            run("::rename note -> note_legacy_v2, note_v3 -> note")?;
        } else {
            run("::rename note_v3 -> note")?;
        }
    }
    if !exists("annotation_removal")? {
        run(schema::CREATE_ANNOTATION_REMOVALS)?;
    }
    run(schema::CREATE_HIGHLIGHT_TRIGGERS)?;
    run(schema::CREATE_NOTE_TRIGGERS)?;
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[3, $now, 'signed note revisions and annotation removals']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}
