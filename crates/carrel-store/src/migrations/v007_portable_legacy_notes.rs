//! Keep unsigned migration ancestry out of private note dependency staging.

use std::collections::BTreeMap;

use carrel_core::annotation::note::NoteRevision;
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::{Result, StoreError};

pub fn up(db: &DbInstance) -> Result<()> {
    let rows = db.run_script(
        "?[id, revision_id, encoded, parents, target_kind, target_id] := *note_incoming{id, revision_id, encoded, parents, target_kind, target_id} :sort id, revision_id",
        BTreeMap::new(), ScriptMutability::Immutable,
    )?;
    // Verify the complete input before changing any rows. Each replacement is
    // atomic; an interrupted upgrade can repeat the remaining replacements.
    let mut replacements = Vec::new();
    for row in rows.rows {
        let DataValue::Bytes(encoded) = &row[2] else {
            return Err(invalid_pending());
        };
        let revision = NoteRevision::from_bytes(encoded)?;
        let kind = revision
            .target_kind
            .as_deref()
            .map_or(DataValue::Null, DataValue::from);
        let target = revision
            .target_id
            .as_deref()
            .map_or(DataValue::Null, DataValue::from);
        if row[0] != DataValue::from(revision.id.as_str())
            || row[1] != DataValue::from(revision.revision_id.as_str())
            || row[4] != kind
            || row[5] != target
        {
            return Err(invalid_pending());
        }
        if revision.signature_version < 3
            && (*encoded != revision.to_bytes()? || row[3] != DataValue::List(Vec::new()))
        {
            replacements.push(revision);
        }
    }
    for revision in replacements {
        replace_pending(db, &revision)?;
    }
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[7, $now, 'portable signed legacy notes without unsigned ancestry']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}

fn replace_pending(db: &DbInstance, revision: &NoteRevision) -> Result<()> {
    let mut params = crate::notes::note_params(revision);
    params.insert("encoded".into(), DataValue::Bytes(revision.to_bytes()?));
    db.run_script(
        r#"
        { ?[id, revision_id] <- [[$id, $revision]] :rm note_incoming {id, revision_id} }
        { ?[id, revision_id, encoded, parents, target_kind, target_id] <- [[$id, $revision, $encoded, $parents, $kind, $target]]
          :put note_incoming {id, revision_id => encoded, parents, target_kind, target_id} }
        "#, params, ScriptMutability::Mutable,
    )?;
    Ok(())
}

fn invalid_pending() -> StoreError {
    StoreError::InvalidInput {
        context: "legacy note migration",
        message: "pending note metadata differs from its verified record".into(),
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::identity::{Keypair, PublicKey, Signature, canonical_bytes, sign_canonical};
    use serde::Serialize;

    use super::*;
    use crate::Store;

    fn legacy(id: &str, version: i64) -> NoteRevision {
        #[derive(Serialize)]
        struct Payload<'a> {
            relation: &'static str,
            id: &'a str,
            body: &'a str,
            target_kind: Option<&'a str>,
            target_id: Option<&'a str>,
            created_by: PublicKey,
            created_at_micros: i64,
            updated_at_micros: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            updated_by: Option<PublicKey>,
            #[serde(skip_serializing_if = "Option::is_none")]
            signature_version: Option<i64>,
        }
        let key = Keypair::from_seed(&[51; 32]);
        let mut revision = NoteRevision {
            id: id.into(),
            revision_id: String::new(),
            body: "Legacy café 日本語".into(),
            target_kind: None,
            target_id: None,
            created_by: *key.public(),
            created_at_micros: 1,
            updated_at_micros: 2,
            updated_by: *key.public(),
            signature_version: version,
            parents: vec!["a".repeat(64)],
            signature: Signature::from_bytes([0; 64]),
        };
        revision.signature = sign_canonical(
            &key,
            &Payload {
                relation: "note",
                id: &revision.id,
                body: &revision.body,
                target_kind: None,
                target_id: None,
                created_by: revision.created_by,
                created_at_micros: 1,
                updated_at_micros: 2,
                updated_by: (version == 2).then_some(*key.public()),
                signature_version: (version == 2).then_some(2),
            },
        )
        .unwrap();
        revision.revision_id = revision.computed_revision_id().unwrap();
        revision
    }

    fn old_store(records: &[NoteRevision]) -> Store {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .query("?[version] <- [[7]] :rm schema_version {version}")
            .unwrap();
        for record in records {
            let mut params = crate::notes::note_params(record);
            // This is the wire encoding produced before normalization existed.
            params.insert(
                "encoded".into(),
                DataValue::Bytes(canonical_bytes(record).unwrap()),
            );
            store.query_with_params("?[id, revision_id, encoded, parents, target_kind, target_id] <- [[$id, $revision, $encoded, $parents, $kind, $target]] :put note_incoming {id, revision_id => encoded, parents, target_kind, target_id}", params).unwrap();
        }
        store
    }

    #[test]
    fn interrupted_normalization_preserves_signatures_and_promotes_missing_unsigned_parents() {
        let records = [legacy("first", 1), legacy("second", 2)];
        for stage in 0..=records.len() {
            let store = old_store(&records);
            for record in records.iter().take(stage) {
                replace_pending(&store.db, &record.clone().into_portable()).unwrap();
            }
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                crate::CURRENT_SCHEMA_VERSION
            );
            for row in store
                .query("?[encoded, parents] := *note_incoming{encoded, parents}")
                .unwrap()
                .rows
            {
                assert_eq!(row[1], DataValue::List(Vec::new()));
                let DataValue::Bytes(encoded) = &row[0] else {
                    panic!("encoded note")
                };
                let decoded = NoteRevision::from_bytes(encoded).unwrap();
                let original = records
                    .iter()
                    .find(|record| record.id == decoded.id)
                    .unwrap();
                assert_eq!(decoded, original.clone().into_portable());
                assert_eq!(decoded.signature, original.signature);
                assert_eq!(decoded.revision_id, original.revision_id);
            }
            store.migrate().unwrap();
            assert_eq!(store.promote_pending_notes().unwrap().applied.len(), 2);
            for record in &records {
                assert_eq!(
                    store.get_note(&record.id).unwrap(),
                    Some(record.clone().into_portable())
                );
            }
        }
    }

    #[test]
    fn invalid_staged_signature_preserves_all_rows_and_leaves_version_unchanged() {
        let good = legacy("a-good", 1);
        let mut bad = legacy("z-bad", 2);
        bad.body.push('!');
        let store = old_store(&[good, bad]);
        let before = store
            .query("?[id, encoded, parents] := *note_incoming{id, encoded, parents}")
            .unwrap()
            .rows;
        assert!(store.migrate().is_err());
        assert_eq!(store.current_schema_version().unwrap(), 6);
        assert_eq!(
            store
                .query("?[id, encoded, parents] := *note_incoming{id, encoded, parents}")
                .unwrap()
                .rows,
            before
        );
    }
}
