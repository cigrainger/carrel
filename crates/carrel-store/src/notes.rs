//! Immutable signed note graphs and verified private dependency staging.

use std::collections::BTreeMap;

use carrel_core::annotation::note::{NoteContent, NoteRevision};
use carrel_core::annotation::{MAX_NOTE_BYTES, next_note_revision};
use carrel_core::identity::{Keypair, PublicKey, Signature, canonical_bytes};
use cozo::{DataValue, Validity};
use serde::Serialize;
use time::OffsetDateTime;

use crate::highlights::NOTE_ALIVE_RULES;
use crate::ids::id_for_authored;
use crate::{Result, Store, StoreError};

/// A verified immutable note version, shared with the pure core.
pub type NoteRecord = NoteRevision;

/// Input for creating a signed note.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PutNoteInput {
    /// Full Markdown body.
    pub body: String,
    /// Optional item/highlight target kind.
    pub target_kind: Option<String>,
    /// Optional stable target identity.
    pub target_id: Option<String>,
    /// Deterministic clock override.
    pub created_at_micros: Option<i64>,
}

/// An ordinary edit against exactly one previously displayed head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditNoteInput {
    /// Stable note identity.
    pub id: String,
    /// Replacement Markdown body.
    pub body: String,
    /// Content-addressed version originally displayed to the editor.
    pub expected_revision_id: String,
    /// Deterministic clock override.
    pub now_micros: Option<i64>,
}

/// Explicit resolution against the complete set of reviewed heads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveNoteInput {
    /// Stable note identity.
    pub id: String,
    /// Chosen or merged full body.
    pub body: String,
    /// Sorted complete set of head identities reviewed by the user.
    pub expected_revision_ids: Vec<String>,
    /// Deterministic clock override.
    pub now_micros: Option<i64>,
}

/// Result of accepting a verified private revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteReceive {
    /// The revision is visible in the causal graph.
    Applied,
    /// An identical accepted revision was delivered again.
    Existing,
    /// The verified revision is durable but its dependencies are unavailable.
    Pending,
    /// A permanent tombstone dominates this delivery.
    Removed,
}

/// Dependency promotion results, containing metadata only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotePromotions {
    /// Note identities whose accepted graph changed.
    pub applied: Vec<String>,
    /// Invalid staged revision identities rejected after dependencies arrived.
    pub rejected: Vec<String>,
}

pub(crate) const NOTE_COLUMNS: &str = "id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version, revision_id, parents";
const WRITE_NOTE: &str = r#"
    { ?[id, revision_id, body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature] <-
        [[$id, $revision, $body, $kind, $target, $creator, $created, $updated, $updater, $version, $parents, $signature]]
      :put note {id, revision_id => body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature} }
    { ?[id, revision_id] <- [[$id, $revision]] :rm note_incoming {id, revision_id} }
"#;

impl Store {
    /// Create a causal root, preserving identical creation retries.
    pub fn put_note(&self, input: PutNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        validate_body(&input.body)?;
        let at = input.created_at_micros.unwrap_or_else(now_micros);
        #[derive(Serialize)]
        struct Identity<'a> {
            body: &'a str,
            target_kind: Option<&'a str>,
            target_id: Option<&'a str>,
        }
        let bytes = canonical_bytes(&Identity {
            body: &input.body,
            target_kind: input.target_kind.as_deref(),
            target_id: input.target_id.as_deref(),
        })
        .map_err(|error| StoreError::Signing {
            context: "note identity",
            message: error.to_string(),
        })?;
        let id = id_for_authored(&bytes, signer.public().as_bytes(), at.div_euclid(1_000_000));
        if let Some(existing) = self.get_note(&id)? {
            return Ok(existing);
        }
        let record = NoteContent {
            id,
            body: input.body,
            target_kind: input.target_kind,
            target_id: input.target_id,
            created_by: *signer.public(),
            created_at_micros: at,
            updated_at_micros: at,
            parents: Vec::new(),
        }
        .sign(signer)
        .map_err(|error| match error {
            carrel_core::annotation::note::NoteRevisionError::Target => StoreError::InvalidInput {
                context: "note.target",
                message: "expected an optional item or highlight target with a nonempty identity"
                    .into(),
            },
            error => error.into(),
        })?;
        self.write_note(&record)?;
        Ok(record)
    }

    /// Return a single living head; unresolved branches are an explicit conflict.
    pub fn get_note(&self, id: &str) -> Result<Option<NoteRecord>> {
        let mut heads = self.note_heads(id)?;
        if heads.len() > 1 {
            return Err(StoreError::AnnotationConflict { id: id.into() });
        }
        Ok(heads.pop())
    }

    /// Read every living head without selecting a conflict winner.
    pub fn note_heads(&self, id: &str) -> Result<Vec<NoteRecord>> {
        let rows = self.query_with_params(&format!(r#"
            {NOTE_ALIVE_RULES}
            ?[{NOTE_COLUMNS}] := *note{{{NOTE_COLUMNS}}}, id = $id, live_note[id], note_head[id, revision_id]
            :sort revision_id
        "#), BTreeMap::from([("id".into(), DataValue::from(id))]))?;
        rows.rows
            .iter()
            .map(|row| decode_note_row(row, 0).map(NoteRecord::into_portable))
            .collect()
    }

    /// Read immutable history, including ancestors hidden by later heads/removal.
    pub fn note_revisions(&self, id: &str) -> Result<Vec<NoteRecord>> {
        let rows = self.query_with_params(
            &format!("?[{NOTE_COLUMNS}] := *note{{{NOTE_COLUMNS}}}, id = $id :sort revision_id"),
            BTreeMap::from([("id".into(), DataValue::from(id))]),
        )?;
        rows.rows
            .iter()
            .map(|row| decode_note_row(row, 0))
            .collect()
    }

    /// Append an ordinary edit only if its exact single head remains current.
    pub fn edit_note(&self, input: EditNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        self.change_note(
            ResolveNoteInput {
                id: input.id,
                body: input.body,
                expected_revision_ids: vec![input.expected_revision_id],
                now_micros: input.now_micros,
            },
            signer,
        )
    }

    /// Append a chosen or merged body superseding the complete reviewed head set.
    pub fn resolve_note(&self, input: ResolveNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        self.change_note(input, signer)
    }

    fn change_note(&self, input: ResolveNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        validate_body(&input.body)?;
        let heads = self.note_heads(&input.id)?;
        let original = heads
            .first()
            .ok_or_else(|| StoreError::AnnotationUnavailable {
                id: input.id.clone(),
            })?;
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(&original.created_by)
        {
            return Err(StoreError::AnnotationUnauthorized { id: input.id });
        }
        let current: Vec<_> = heads
            .iter()
            .map(|record| record.revision_id.clone())
            .collect();
        if current != input.expected_revision_ids {
            return Err(StoreError::AnnotationConflict { id: input.id });
        }
        let latest = heads
            .iter()
            .map(|record| record.updated_at_micros)
            .max()
            .expect("nonempty heads");
        // Times are display metadata, so saturation must not prevent a causal
        // edit. Signed parents keep revisions distinct even at i64::MAX.
        let at = next_note_revision(input.now_micros.unwrap_or_else(now_micros), latest)
            .unwrap_or(latest);
        let record = NoteContent {
            id: original.id.clone(),
            body: input.body,
            target_kind: original.target_kind.clone(),
            target_id: original.target_id.clone(),
            created_by: original.created_by,
            created_at_micros: original.created_at_micros,
            updated_at_micros: at,
            parents: current,
        }
        .sign(signer)?;
        record.validate_parents(&heads)?;
        self.write_note(&record)?;
        Ok(record)
    }

    /// Read all living heads attached to a target, preserving unresolved versions.
    pub fn list_notes_for_target(&self, kind: &str, target: &str) -> Result<Vec<NoteRecord>> {
        let rows = self.query_with_params(
            &format!(
                r#"
            {NOTE_ALIVE_RULES}
            ?[{NOTE_COLUMNS}] := *note{{{NOTE_COLUMNS}}}, target_kind = $kind, target_id = $target,
                live_note[id], note_head[id, revision_id]
            :sort updated_at, id, revision_id
        "#
            ),
            BTreeMap::from([
                ("kind".into(), DataValue::from(kind)),
                ("target".into(), DataValue::from(target)),
            ]),
        )?;
        rows.rows
            .iter()
            .map(|row| decode_note_row(row, 0).map(NoteRecord::into_portable))
            .collect()
    }

    /// Accept a verified revision from an authenticated, currently trusted sender.
    ///
    /// The bridge supplies the authenticated connection identity. This API never
    /// grants trust from a key embedded in the incoming revision itself.
    pub fn accept_self_note_revision(
        &self,
        record: &NoteRecord,
        sender: &PublicKey,
    ) -> Result<NoteReceive> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        record.verify_signature()?;
        let devices = self.self_devices()?;
        if !devices
            .iter()
            .any(|device| device.certificate.device_pubkey == *sender && device.is_authorized())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        if ![record.created_by, record.updated_by].iter().all(|key| {
            devices
                .iter()
                .any(|device| device.certificate.device_pubkey == *key)
        }) {
            return Err(StoreError::IdentityMasterMismatch);
        }
        self.accept_note_unlocked(record)
    }

    fn accept_note_unlocked(&self, record: &NoteRecord) -> Result<NoteReceive> {
        record.verify_signature()?;
        let portable = record.clone().into_portable();
        let record = &portable;
        let dependencies = self.note_dependencies(record)?;
        if dependencies == Dependencies::Removed {
            self.discard_pending(&record.id, &record.revision_id)?;
            return Ok(NoteReceive::Removed);
        }
        let existing = self.query_with_params(
            &format!(
                "?[{NOTE_COLUMNS}] := *note{{{NOTE_COLUMNS}}}, id = $id, revision_id = $revision"
            ),
            BTreeMap::from([
                ("id".into(), DataValue::from(record.id.as_str())),
                (
                    "revision".into(),
                    DataValue::from(record.revision_id.as_str()),
                ),
            ]),
        )?;
        if let Some(row) = existing.rows.first() {
            if decode_note_row(row, 0)?.into_portable() != *record {
                return Err(StoreError::InvalidInput {
                    context: "note revision",
                    message: "immutable revision differs from an existing record".into(),
                });
            }
            return Ok(NoteReceive::Existing);
        }
        let previous = self.query_with_params(
            &format!("?[{NOTE_COLUMNS}] := *note{{{NOTE_COLUMNS}}}, id = $id :limit 1"),
            BTreeMap::from([("id".into(), DataValue::from(record.id.as_str()))]),
        )?;
        if previous
            .rows
            .first()
            .map(|row| decode_note_row(row, 0))
            .transpose()?
            .is_some_and(|existing| !same_identity(&existing, record))
        {
            return Err(carrel_core::annotation::note::NoteRevisionError::ImmutableMetadata.into());
        }
        let rows = self.query_with_params(&format!("wanted[revision_id] <- $parents ?[{NOTE_COLUMNS}] := wanted[revision_id], *note{{{NOTE_COLUMNS}}}, id = $id :sort revision_id"),
            BTreeMap::from([("id".into(), DataValue::from(record.id.as_str())),
                ("parents".into(), DataValue::List(record.parents.iter().map(|id| DataValue::List(vec![DataValue::from(id.as_str())])).collect()))]))?;
        let parents: Vec<_> = rows
            .rows
            .iter()
            .map(|row| decode_note_row(row, 0))
            .collect::<Result<_>>()?;
        if parents.len() != record.parents.len() || dependencies == Dependencies::Missing {
            self.query_with_params("?[id, revision_id, encoded, parents, target_kind, target_id] <- [[$id, $revision, $encoded, $parents, $kind, $target]] :put note_incoming {id, revision_id => encoded, parents, target_kind, target_id}",
                BTreeMap::from([
                    ("id".into(), DataValue::from(record.id.as_str())), ("revision".into(), DataValue::from(record.revision_id.as_str())),
                    ("encoded".into(), DataValue::Bytes(record.to_bytes()?)), ("parents".into(), list_value(&record.parents)),
                    ("kind".into(), optional_value(record.target_kind.as_deref())), ("target".into(), optional_value(record.target_id.as_deref())),
                ]))?;
            return Ok(NoteReceive::Pending);
        }
        record.validate_parents(&parents)?;
        self.write_note(record)?;
        Ok(NoteReceive::Applied)
    }

    /// Promote ready private revisions in bounded batches after any dependency write.
    ///
    /// An accepted pending version remains historical work after its signing
    /// device is revoked. New deliveries still require a current trusted sender.
    pub fn promote_pending_notes(&self) -> Result<NotePromotions> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        let mut report = NotePromotions::default();
        loop {
            let rows = self.query(&format!(r#"
                {NOTE_ALIVE_RULES}
                missing[id, rev] := *note_incoming{{id, revision_id: rev, parents}}, parent in parents, not *note{{id, revision_id: parent}}
                removed[id, rev] := *note_incoming{{id, revision_id: rev}}, *entity_tombstone{{id}}
                removed[id, rev] := *note_incoming{{id, revision_id: rev, target_id: target}}, *entity_tombstone{{id: target}}
                removed[id, rev] := *note_incoming{{id, revision_id: rev, target_kind: 'highlight', target_id: target}}, *highlight{{id: target, item_id}}, *entity_tombstone{{id: item_id}}
                ready[id, rev] := *note_incoming{{id, revision_id: rev, target_kind: kind}}, is_null(kind), not missing[id, rev]
                ready[id, rev] := *note_incoming{{id, revision_id: rev, target_kind: 'item', target_id: target}}, live_item[target], not missing[id, rev]
                ready[id, rev] := *note_incoming{{id, revision_id: rev, target_kind: 'highlight', target_id: target}}, live_highlight[target], not missing[id, rev]
                ready[id, rev] := removed[id, rev]
                ?[id, rev, encoded] := *note_incoming{{id, revision_id: rev, encoded}}, ready[id, rev]
                :sort id, rev
                :limit 100
            "#))?;
            if rows.rows.is_empty() {
                break;
            }
            for row in rows.rows {
                let id = string(&row[0])?;
                let rev = string(&row[1])?;
                let decoded = NoteRecord::from_bytes(bytes(&row[2])?);
                let outcome = decoded.map_err(StoreError::from).and_then(|record| {
                    if record.id != id || record.revision_id != rev {
                        return Err(StoreError::InvalidInput {
                            context: "pending note",
                            message: "record identity differs from staging keys".into(),
                        });
                    }
                    self.accept_note_unlocked(&record)
                });
                match outcome {
                    Ok(NoteReceive::Applied) => report.applied.push(id),
                    Ok(NoteReceive::Removed | NoteReceive::Existing) => {
                        self.discard_pending(&id, &rev)?
                    }
                    Ok(NoteReceive::Pending) => {
                        return Err(StoreError::InvalidInput {
                            context: "pending note",
                            message: "staging dependency metadata differs from its record".into(),
                        });
                    }
                    Err(StoreError::Cozo(error)) => return Err(StoreError::Cozo(error)),
                    Err(_) => {
                        self.discard_pending(&id, &rev)?;
                        report.rejected.push(rev);
                    }
                }
            }
        }
        report.applied.sort();
        report.applied.dedup();
        Ok(report)
    }

    fn note_dependencies(&self, record: &NoteRecord) -> Result<Dependencies> {
        let rows = self.query_with_params(&format!(r#"
            {NOTE_ALIVE_RULES}
            removed[id] := *entity_tombstone{{id}}, id = $id
            removed[id] := *entity_tombstone{{id: target}}, target = $target, id = $id
            removed[id] := *highlight{{id: target, item_id}}, target = $target, *entity_tombstone{{id: item_id}}, id = $id
            alive[id] := live_item[id], $kind == 'item'
            alive[id] := live_highlight[id], $kind == 'highlight'
            status[flag] := removed[_], flag = 'removed'
            status[flag] := is_null($kind), flag = 'ready'
            status[flag] := alive[target], target = $target, flag = 'ready'
            ?[flag] := status[flag]
        "#), BTreeMap::from([
            ("id".into(), DataValue::from(record.id.as_str())),
            ("kind".into(), optional_value(record.target_kind.as_deref())), ("target".into(), optional_value(record.target_id.as_deref())),
        ]))?;
        if rows
            .rows
            .iter()
            .any(|row| row[0] == DataValue::from("removed"))
        {
            return Ok(Dependencies::Removed);
        }
        if rows
            .rows
            .iter()
            .any(|row| row[0] == DataValue::from("ready"))
        {
            return Ok(Dependencies::Ready);
        }
        Ok(Dependencies::Missing)
    }

    fn write_note(&self, record: &NoteRecord) -> Result<()> {
        let mut params = note_params(record);
        params.extend(crate::private_library::private_fact_params(
            &carrel_core::private_library::PrivateFact::Note(record.clone()),
        )?);
        self.query_with_params(
            &format!(
                "{WRITE_NOTE}\n{}",
                crate::private_library::QUEUE_PRIVATE_FACT
            ),
            params,
        )?;
        Ok(())
    }

    fn discard_pending(&self, id: &str, revision: &str) -> Result<()> {
        self.query_with_params(
            "?[id, revision_id] <- [[$id, $revision]] :rm note_incoming {id, revision_id}",
            BTreeMap::from([
                ("id".into(), DataValue::from(id)),
                ("revision".into(), DataValue::from(revision)),
            ]),
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Dependencies {
    Ready,
    Missing,
    Removed,
}

fn same_identity(a: &NoteRecord, b: &NoteRecord) -> bool {
    a.id == b.id
        && a.created_by == b.created_by
        && a.created_at_micros == b.created_at_micros
        && a.target_kind == b.target_kind
        && a.target_id == b.target_id
}

fn validate_body(body: &str) -> Result<()> {
    if body.trim().is_empty() || body.len() > MAX_NOTE_BYTES {
        return Err(StoreError::InvalidInput {
            context: "note.body",
            message: format!("note must contain text and be at most {MAX_NOTE_BYTES} bytes"),
        });
    }
    Ok(())
}

pub(crate) fn note_params(record: &NoteRecord) -> BTreeMap<String, DataValue> {
    BTreeMap::from([
        ("id".into(), DataValue::from(record.id.as_str())),
        (
            "revision".into(),
            DataValue::from(record.revision_id.as_str()),
        ),
        ("body".into(), DataValue::from(record.body.as_str())),
        ("kind".into(), optional_value(record.target_kind.as_deref())),
        ("target".into(), optional_value(record.target_id.as_deref())),
        (
            "creator".into(),
            DataValue::Bytes(record.created_by.to_bytes().to_vec()),
        ),
        (
            "created".into(),
            DataValue::Validity(Validity::from((record.created_at_micros, true))),
        ),
        (
            "updated".into(),
            DataValue::Validity(Validity::from((record.updated_at_micros, true))),
        ),
        (
            "updater".into(),
            DataValue::Bytes(record.updated_by.to_bytes().to_vec()),
        ),
        ("version".into(), DataValue::from(record.signature_version)),
        ("parents".into(), list_value(&record.parents)),
        (
            "signature".into(),
            DataValue::Bytes(record.signature.to_bytes().to_vec()),
        ),
    ])
}

pub(crate) fn decode_note_row(row: &[DataValue], offset: usize) -> Result<NoteRecord> {
    let mut record = decode_legacy_note_row(row, offset)?;
    record.revision_id = string(&row[offset + 10])?;
    let DataValue::List(parents) = &row[offset + 11] else {
        return Err(unexpected("note parents"));
    };
    record.parents = parents.iter().map(string).collect::<Result<_>>()?;
    record.verify_signature()?;
    Ok(record)
}

pub(crate) fn decode_legacy_note_row(row: &[DataValue], offset: usize) -> Result<NoteRecord> {
    let at = |index| -> Result<i64> {
        let DataValue::Validity(value) = &row[offset + index] else {
            return Err(unexpected("note time"));
        };
        Ok(value.timestamp.0.0)
    };
    let DataValue::Num(cozo::Num::Int(version)) = row[offset + 9] else {
        return Err(unexpected("note version"));
    };
    let mut record = NoteRecord {
        id: string(&row[offset])?,
        revision_id: String::new(),
        body: string(&row[offset + 1])?,
        target_kind: optional_string(&row[offset + 2])?,
        target_id: optional_string(&row[offset + 3])?,
        created_by: PublicKey::from_slice(bytes(&row[offset + 4])?)
            .map_err(|_| unexpected("note creator"))?,
        created_at_micros: at(5)?,
        updated_at_micros: at(6)?,
        signature: Signature::from_slice(bytes(&row[offset + 7])?)
            .map_err(|_| unexpected("note signature"))?,
        updated_by: PublicKey::from_slice(bytes(&row[offset + 8])?)
            .map_err(|_| unexpected("note updater"))?,
        signature_version: version,
        parents: Vec::new(),
    };
    record.revision_id = record.computed_revision_id()?;
    Ok(record)
}

fn list_value(values: &[String]) -> DataValue {
    DataValue::List(
        values
            .iter()
            .map(|value| DataValue::from(value.as_str()))
            .collect(),
    )
}
fn optional_value(value: Option<&str>) -> DataValue {
    value.map_or(DataValue::Null, DataValue::from)
}
fn optional_string(value: &DataValue) -> Result<Option<String>> {
    if *value == DataValue::Null {
        Ok(None)
    } else {
        string(value).map(Some)
    }
}
fn string(value: &DataValue) -> Result<String> {
    if let DataValue::Str(value) = value {
        Ok(value.to_string())
    } else {
        Err(unexpected("note string"))
    }
}
fn bytes(value: &DataValue) -> Result<&[u8]> {
    if let DataValue::Bytes(value) = value {
        Ok(value)
    } else {
        Err(unexpected("note bytes"))
    }
}
fn unexpected(context: &'static str) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: "invalid note column type".into(),
    }
}
fn now_micros() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64
}
