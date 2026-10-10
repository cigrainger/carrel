//! Preserve old signing payloads while replacing timestamp revision identities.

use std::collections::BTreeMap;

use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::notes::{decode_legacy_note_row, note_params};
use crate::{Result, schema};

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    let exists = |name: &str| -> Result<bool> {
        Ok(run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(name))))
    };
    let causal = exists("note")?
        && run("::columns note")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from("revision_id")));
    if !causal {
        if !exists("note_v5")? {
            run(schema::CREATE_CAUSAL_NOTES)?;
        }
        let source = if exists("note_legacy_v3")? {
            "note_legacy_v3"
        } else {
            "note"
        };
        let rows = run(&format!(
            "?[id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version] := *{source}{{id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version}} :sort id, -updated_at"
        ))?;
        let mut previous: BTreeMap<String, String> = BTreeMap::new();
        for row in rows.rows {
            let mut record = decode_legacy_note_row(&row, 0)?;
            if let Some(parent) = previous.get(&record.id) {
                record.parents.push(parent.clone());
            }
            record.verify_signature()?;
            db.run_script(r#"
                ?[id, revision_id, body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature] <-
                    [[$id, $revision, $body, $kind, $target, $creator, $created, $updated, $updater, $version, $parents, $signature]]
                :put note_v5 {id, revision_id => body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature}
            "#, note_params(&record), ScriptMutability::Mutable)?;
            previous.insert(record.id, record.revision_id);
        }
        if exists("note")? {
            run("::rename note -> note_legacy_v3, note_v5 -> note")?;
        } else {
            run("::rename note_v5 -> note")?;
        }
    }
    if !exists("note_incoming")? {
        run(schema::CREATE_INCOMING_NOTES)?;
    }
    run(schema::CREATE_CAUSAL_NOTE_GUARDS)?;
    run(schema::CREATE_INCOMING_NOTE_GUARDS)?;
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000) as i64;
    db.run_script("?[version, applied_at, description] <- [[5, $now, 'content-addressed causal note revisions']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]), ScriptMutability::Mutable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use carrel_core::annotation::note::NoteRevision;
    use carrel_core::identity::{Keypair, PublicKey, Signature, sign_canonical};
    use serde::Serialize;

    fn old_store() -> Store {
        let store = Store::open_in_memory().unwrap();
        super::super::v001_initial::up(&store.db).unwrap();
        super::super::v002_search::up(&store.db).unwrap();
        super::super::v003_annotations::up(&store.db).unwrap();
        super::super::v004_identity::up(&store.db).unwrap();
        store
    }

    fn legacy(id: &str, at: i64, version: i64) -> NoteRevision {
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
        let creator = Keypair::from_seed(&[61; 32]);
        let updater = Keypair::from_seed(&[62; 32]);
        let signer = if version == 1 { &creator } else { &updater };
        let mut record = NoteRevision {
            id: id.into(),
            revision_id: String::new(),
            body: format!("Legacy café 日本語 {at}"),
            target_kind: None,
            target_id: None,
            created_by: *creator.public(),
            created_at_micros: 1,
            updated_at_micros: at,
            updated_by: *signer.public(),
            signature_version: version,
            parents: Vec::new(),
            signature: Signature::from_bytes([0; 64]),
        };
        record.signature = sign_canonical(
            signer,
            &Payload {
                relation: "note",
                id: &record.id,
                body: &record.body,
                target_kind: None,
                target_id: None,
                created_by: record.created_by,
                created_at_micros: 1,
                updated_at_micros: at,
                updated_by: (version == 2).then_some(record.updated_by),
                signature_version: (version == 2).then_some(version),
            },
        )
        .unwrap();
        record.revision_id = record.computed_revision_id().unwrap();
        record.verify_signature().unwrap();
        record
    }

    fn put_legacy(store: &Store, record: &NoteRevision) {
        store.query_with_params(r#"
            ?[id, updated_at, body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature] <-
                [[$id, $updated, $body, $kind, $target, $creator, $created, $updater, $version, $signature]]
            :put note {id, updated_at => body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature}
        "#, note_params(record)).unwrap();
    }

    fn put_staged(store: &Store, record: &NoteRevision) {
        store.query_with_params(r#"
            ?[id, revision_id, body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature] <-
                [[$id, $revision, $body, $kind, $target, $creator, $created, $updated, $updater, $version, $parents, $signature]]
            :put note_v5 {id, revision_id => body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature}
        "#, note_params(record)).unwrap();
    }

    #[test]
    fn every_upgrade_stage_preserves_legacy_signatures_history_and_tombstones() {
        for stage in 0..=5 {
            let store = old_store();
            let mut records = vec![
                legacy("old", 2, 1),
                legacy("old", 4, 2),
                legacy("old", 8, 2),
            ];
            let removed = legacy("removed", 2, 1);
            for record in records.iter().chain([&removed]) {
                put_legacy(&store, record);
            }
            store.query("?[id, tombstoned_at, reason] <- [['removed', validity(1), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
            for index in 1..records.len() {
                records[index].parents = vec![records[index - 1].revision_id.clone()];
            }
            if stage >= 1 {
                store.query(schema::CREATE_CAUSAL_NOTES).unwrap();
            }
            if stage >= 2 {
                put_staged(&store, &records[0]);
            }
            if stage >= 3 {
                for record in records.iter().chain([&removed]) {
                    put_staged(&store, record);
                }
                store
                    .query("::rename note -> note_legacy_v3, note_v5 -> note")
                    .unwrap();
            }
            if stage >= 4 {
                store.query(schema::CREATE_INCOMING_NOTES).unwrap();
            }
            if stage >= 5 {
                store.query(schema::CREATE_CAUSAL_NOTE_GUARDS).unwrap();
                store.query(schema::CREATE_INCOMING_NOTE_GUARDS).unwrap();
            }
            store.migrate().unwrap();
            assert_eq!(
                store.current_schema_version().unwrap(),
                crate::CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                store.get_note("old").unwrap(),
                Some(records[2].clone().into_portable())
            );
            let history = store.note_revisions("old").unwrap();
            assert_eq!(history.len(), 3);
            for record in &records {
                assert!(history.contains(record));
                record.verify_signature().unwrap();
            }
            assert_eq!(store.get_note("removed").unwrap(), None);
            assert_eq!(store.note_revisions("removed").unwrap(), vec![removed]);
            assert_eq!(
                store
                    .query("?[id, at] := *note_legacy_v3{id, updated_at: at}")
                    .unwrap()
                    .rows
                    .len(),
                4
            );
            store.migrate().unwrap();
            assert_eq!(store.note_revisions("old").unwrap(), history);
        }
    }

    #[test]
    fn invalid_legacy_signature_leaves_original_table_and_upgrade_version_intact() {
        let store = old_store();
        let mut record = legacy("bad", 2, 1);
        record.body.push('!');
        put_legacy(&store, &record);
        assert!(store.migrate().is_err());
        assert_eq!(store.current_schema_version().unwrap(), 4);
        assert_eq!(
            store
                .query("?[body] := *note{id: 'bad', body}")
                .unwrap()
                .rows[0][0],
            DataValue::from(record.body)
        );
        assert!(
            store
                .query("::columns note")
                .unwrap()
                .rows
                .iter()
                .all(|row| row[0] != DataValue::from("revision_id"))
        );
    }
}
