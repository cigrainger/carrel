//! Typed highlight and note persistence APIs.

use std::collections::BTreeMap;

use carrel_core::annotation::{
    AnnotationCursor, AnnotationPage, HighlightLocation, MAX_NOTE_BYTES, next_note_revision,
};
use carrel_core::identity::{
    Keypair, PublicKey, Signature, canonical_bytes, sign_canonical, verify_canonical,
};
use cozo::{DataValue, JsonData};
use serde::Serialize;
use time::OffsetDateTime;

use crate::ids::id_for_authored;
use crate::{Result, Store, StoreError};

/// Removals dominate revision time and cascade to attached notes.
pub(crate) const NOTE_ALIVE_RULES: &str = r#"
    live_item[id] := *item{id}, not *entity_tombstone{id}
    live_highlight[id] := *highlight{id, item_id}, live_item[item_id], not *entity_tombstone{id}
    live_note[id] := *note{id, target_kind, target_id @ "END"},
        is_null(target_kind), is_null(target_id), not *entity_tombstone{id}
    live_note[id] := *note{id, target_kind: 'item', target_id @ "END"},
        live_item[target_id], not *entity_tombstone{id}
    live_note[id] := *note{id, target_kind: 'highlight', target_id @ "END"},
        live_highlight[target_id], not *entity_tombstone{id}
"#;

fn validate_note_body(body: &str) -> Result<()> {
    if body.trim().is_empty() || body.len() > MAX_NOTE_BYTES {
        return Err(StoreError::InvalidInput {
            context: "note.body",
            message: format!("note must contain text and be at most {MAX_NOTE_BYTES} bytes"),
        });
    }
    Ok(())
}

fn validate_note_target(kind: Option<&str>, id: Option<&str>) -> Result<()> {
    if !matches!(
        (kind, id),
        (None, None) | (Some("item" | "highlight"), Some(_))
    ) || id.is_some_and(|id| id.is_empty())
    {
        return Err(StoreError::InvalidInput {
            context: "note.target",
            message: "target must be an existing item, highlight, or absent".into(),
        });
    }
    Ok(())
}

/// Input for creating a highlight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PutHighlightInput {
    /// Item being highlighted.
    pub item_id: String,
    /// Location of the selected text.
    pub location: HighlightLocation,
    /// Optional human-readable location label, such as a page or section.
    pub location_label: Option<String>,
    /// Optional user-selected color token.
    pub color: Option<String>,
    /// Optional timestamp override for deterministic tests.
    pub created_at_micros: Option<i64>,
}

/// Stored highlight row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightRecord {
    /// Stable authored-fact id.
    pub id: String,
    /// Item being highlighted.
    pub item_id: String,
    /// Exact selected text.
    pub quoted_text: String,
    /// Structured selector location.
    pub location: HighlightLocation,
    /// Optional human-readable location label.
    pub location_label: Option<String>,
    /// Optional user-selected color token.
    pub color: Option<String>,
    /// Device public key that created the fact.
    pub created_by: PublicKey,
    /// Creation time in Unix microseconds.
    pub created_at_micros: i64,
    /// Ed25519 signature over the canonical highlight payload.
    pub signature: Signature,
}

impl HighlightRecord {
    /// Verify the immutable persisted highlight payload.
    pub fn verify_signature(&self) -> Result<()> {
        verify_canonical(
            &self.created_by,
            &highlight_signature_payload_from_record(self),
            &self.signature,
        )
        .map_err(|error| StoreError::Signing {
            context: "highlight verification",
            message: error.to_string(),
        })
    }
}

/// Signed private removal, accompanied by a permanent entity tombstone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationRemovalRecord {
    /// Removed entity identity.
    pub id: String,
    /// Removed entity kind.
    pub kind: String,
    /// Device authorizing the removal.
    pub removed_by: PublicKey,
    /// Removal time in Unix microseconds.
    pub removed_at_micros: i64,
    /// Canonical payload signature.
    pub signature: Signature,
}

#[derive(Serialize)]
struct RemovalPayload<'a> {
    relation: &'static str,
    id: &'a str,
    kind: &'a str,
    removed_by: PublicKey,
    removed_at_micros: i64,
}

impl AnnotationRemovalRecord {
    fn payload(&self) -> RemovalPayload<'_> {
        RemovalPayload {
            relation: "annotation_removal",
            id: &self.id,
            kind: &self.kind,
            removed_by: self.removed_by,
            removed_at_micros: self.removed_at_micros,
        }
    }

    /// Verify the entity, kind, time, and author of this removal.
    pub fn verify_signature(&self) -> Result<()> {
        verify_canonical(&self.removed_by, &self.payload(), &self.signature).map_err(|error| {
            StoreError::Signing {
                context: "annotation removal verification",
                message: error.to_string(),
            }
        })
    }
}

/// Highlight row with enough item context for library-level lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightWithItem {
    /// Highlight fact.
    pub highlight: HighlightRecord,
    /// Current item title.
    pub item_title: String,
    /// Current item URL, if known.
    pub item_primary_url: Option<String>,
}

/// A review/export row assembled in one database snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationReview {
    /// Immutable highlight and article metadata.
    pub source: HighlightWithItem,
    /// Publisher label when available.
    pub site_name: Option<String>,
    /// All current attached notes, newest revision first.
    pub notes: Vec<NoteRecord>,
}

/// Input for creating a note.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PutNoteInput {
    /// Note body.
    pub body: String,
    /// Optional target kind, for this brief usually `highlight`.
    pub target_kind: Option<String>,
    /// Optional target id matching `target_kind`.
    pub target_id: Option<String>,
    /// Optional timestamp override for deterministic tests.
    pub created_at_micros: Option<i64>,
}

/// Stored note row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteRecord {
    /// Stable authored-fact id.
    pub id: String,
    /// Note body.
    pub body: String,
    /// Optional target kind.
    pub target_kind: Option<String>,
    /// Optional target id.
    pub target_id: Option<String>,
    /// Device public key that created the fact.
    pub created_by: PublicKey,
    /// Creation time in Unix microseconds.
    pub created_at_micros: i64,
    /// Last update time in Unix microseconds.
    pub updated_at_micros: i64,
    /// Ed25519 signature over the canonical note payload.
    pub signature: Signature,
    /// Device that signed this revision.
    pub updated_by: PublicKey,
    /// Canonical payload format, preserving legacy signatures.
    pub signature_version: i64,
}

/// An optimistic edit against a specific, previously read revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditNoteInput {
    /// Stable note identity.
    pub id: String,
    /// Replacement Markdown body.
    pub body: String,
    /// Revision the editor originally read.
    pub expected_updated_at_micros: i64,
    /// Clock override for deterministic tests.
    pub now_micros: Option<i64>,
}

impl NoteRecord {
    /// Verify the stored revision with its recorded updating device.
    pub fn verify_signature(&self) -> Result<()> {
        if !matches!(self.signature_version, 1 | 2)
            || (self.signature_version == 1 && self.updated_by != self.created_by)
        {
            return Err(StoreError::InvalidInput {
                context: "note signature version",
                message: "unsupported signature version or updater".into(),
            });
        }
        verify_canonical(
            &self.updated_by,
            &note_signature_payload_from_record(self),
            &self.signature,
        )
        .map_err(|error| StoreError::Signing {
            context: "note verification",
            message: error.to_string(),
        })
    }
}

impl Store {
    /// Browse the entire living highlight archive without truncating attached notes.
    pub fn annotation_page(
        &self,
        limit: usize,
        after: Option<&AnnotationCursor>,
    ) -> Result<AnnotationPage<AnnotationReview>> {
        let limit = limit.clamp(1, 100);
        let mut items = self.annotation_snapshot(Some(limit + 1), after)?;
        let next_cursor = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|row| AnnotationCursor {
                created_at_micros: row.source.highlight.created_at_micros,
                id: row.source.highlight.id.clone(),
            })
        } else {
            None
        };
        Ok(AnnotationPage { items, next_cursor })
    }

    /// Read every living highlight and latest note in one coherent export snapshot.
    pub fn annotation_export_snapshot(&self) -> Result<Vec<AnnotationReview>> {
        self.annotation_snapshot(None, None)
    }

    fn annotation_snapshot(
        &self,
        limit: Option<usize>,
        after: Option<&AnnotationCursor>,
    ) -> Result<Vec<AnnotationReview>> {
        let selection = if limit.is_some() {
            "selected[rank, sort_at, id] <~ ReorderSort(eligible[id, sort_at], out: [sort_at, id], sort_by: [validity(sort_at), id], take: $limit)"
        } else {
            "selected[rank, sort_at, id] := eligible[id, sort_at], rank = 0"
        };
        let rows = self.query_with_params(&format!(r#"
            {NOTE_ALIVE_RULES}
            eligible[id, sort_at] := live_highlight[id], *highlight{{id, created_at}}, sort_at = to_int(created_at),
                (!$has_after || sort_at < $after_time || (sort_at == $after_time && id > $after_id))
            {selection}
            publisher[item_id, site] := *item_article{{item_id, site_name: site}}
            publisher[item_id, site] := *item{{id: item_id}}, not *item_article{{item_id}}, site = null
            note_rows[h, n, body, kind, target, author, created, updated, sig, updater, version] :=
                selected[_, _, h], *note{{id: n, body, target_kind: kind, target_id: target, created_by: author, created_at: created, updated_at: updated, signature: sig, updated_by: updater, signature_version: version @ "END"}},
                kind = 'highlight', target = h, live_note[n]
            has_note[h] := note_rows[h, _, _, _, _, _, _, _, _, _, _]
            attached[h, n, body, kind, target, author, created, updated, sig, updater, version] :=
                note_rows[h, n, body, kind, target, author, created, updated, sig, updater, version]
            attached[h, n, body, kind, target, author, created, updated, sig, updater, version] := selected[_, _, h], not has_note[h],
                n = null, body = null, kind = null, target = null, author = null, created = null, updated = null, sig = null, updater = null, version = null
            ?[id, item_id, quote, location, label, color, author, created, sig, title, url, site, n, body, kind, target, nauthor, ncreated, updated, nsig, updater, version] :=
                selected[_, _, id], *highlight{{id, item_id, quoted_text: quote, location, location_label: label, color, created_by: author, created_at: created, signature: sig}},
                *item{{id: item_id, title, primary_url: url}}, publisher[item_id, site],
                attached[id, n, body, kind, target, nauthor, ncreated, updated, nsig, updater, version]
            :sort created, id, updated, n
        "#), BTreeMap::from([
            ("limit".into(), DataValue::from(i64::try_from(limit.unwrap_or(1)).expect("bounded page"))),
            ("has_after".into(), DataValue::Bool(after.is_some())),
            ("after_time".into(), DataValue::from(after.map_or(0, |cursor| cursor.created_at_micros))),
            ("after_id".into(), DataValue::from(after.map_or("", |cursor| cursor.id.as_str()))),
        ]))?;
        let mut result: Vec<AnnotationReview> = Vec::new();
        for row in &rows.rows {
            let id = value_as_string(required(row, 0, "highlight.id")?)?;
            if result
                .last()
                .is_none_or(|previous| previous.source.highlight.id != id)
            {
                result.push(AnnotationReview {
                    source: HighlightWithItem {
                        highlight: decode_highlight_row(row, 0)?,
                        item_title: value_as_string(required(row, 9, "item.title")?)?,
                        item_primary_url: optional_string(required(row, 10, "item.primary_url")?)?,
                    },
                    site_name: optional_string(required(row, 11, "item_article.site_name")?)?,
                    notes: Vec::new(),
                });
            }
            if !matches!(required(row, 12, "note.id")?, DataValue::Null) {
                result
                    .last_mut()
                    .expect("highlight added above")
                    .notes
                    .push(decode_note_row(row, 12)?);
            }
        }
        Ok(result)
    }

    /// Create an immutable highlight; an identical creation retry is idempotent.
    pub fn put_highlight(
        &self,
        input: PutHighlightInput,
        signer: &Keypair,
    ) -> Result<HighlightRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        input
            .location
            .validate()
            .map_err(|source| StoreError::InvalidInput {
                context: "highlight.location",
                message: source.to_string(),
            })?;
        let quoted_text = input.location.quoted_text().to_string();
        let created_at_micros = input.created_at_micros.unwrap_or_else(now_micros);
        let created_by = *signer.public();
        let id_payload = HighlightIdPayload {
            item_id: &input.item_id,
            quoted_text: &quoted_text,
            location: &input.location,
        };
        let id_content = canonical_bytes(&id_payload).map_err(|source| StoreError::Signing {
            context: "highlight id payload",
            message: source.to_string(),
        })?;
        let id = id_for_authored(
            &id_content,
            created_by.as_bytes(),
            created_at_micros.div_euclid(1_000_000),
        );
        let unsigned = UnsignedHighlight {
            id,
            item_id: input.item_id,
            quoted_text,
            location: input.location,
            location_label: input.location_label,
            color: input.color,
            created_by,
            created_at_micros,
        };
        if let Some(existing) = self.get_highlight(&unsigned.id)? {
            if existing.item_id == unsigned.item_id
                && existing.location == unsigned.location
                && existing.color == unsigned.color
                && existing.location_label == unsigned.location_label
            {
                return Ok(existing);
            }
            return Err(StoreError::InvalidInput {
                context: "highlight identity",
                message: "immutable highlight identity already exists".into(),
            });
        }
        let signature = sign_highlight(&unsigned, signer)?;

        self.query_with_params(
            r#"
            ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] :=
                id = $id,
                item_id = $item_id,
                quoted_text = $quoted_text,
                location = $location,
                location_label = $location_label,
                color = $color,
                created_by = $created_by,
                created_at = $created_at,
                signature = $signature
            :put highlight {id => item_id, quoted_text, location, location_label, color, created_by, created_at, signature}
            "#,
            BTreeMap::from([
                ("id".to_string(), DataValue::from(unsigned.id.as_str())),
                (
                    "item_id".to_string(),
                    DataValue::from(unsigned.item_id.as_str()),
                ),
                (
                    "quoted_text".to_string(),
                    DataValue::from(unsigned.quoted_text.as_str()),
                ),
                (
                    "location".to_string(),
                    DataValue::Json(JsonData(location_json(&unsigned.location)?)),
                ),
                (
                    "location_label".to_string(),
                    option_string(unsigned.location_label.as_deref()),
                ),
                (
                    "color".to_string(),
                    option_string(unsigned.color.as_deref()),
                ),
                (
                    "created_by".to_string(),
                    DataValue::Bytes(unsigned.created_by.as_bytes().to_vec()),
                ),
                (
                    "created_at".to_string(),
                    validity_micros(unsigned.created_at_micros),
                ),
                (
                    "signature".to_string(),
                    DataValue::Bytes(signature.as_bytes().to_vec()),
                ),
            ]),
        )?;

        Ok(HighlightRecord {
            id: unsigned.id,
            item_id: unsigned.item_id,
            quoted_text: unsigned.quoted_text,
            location: unsigned.location,
            location_label: unsigned.location_label,
            color: unsigned.color,
            created_by: unsigned.created_by,
            created_at_micros: unsigned.created_at_micros,
            signature,
        })
    }

    /// Return a highlight by id.
    pub fn get_highlight(&self, id: &str) -> Result<Option<HighlightRecord>> {
        let rows = self.query_with_params(
            r#"
            ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] :=
                *highlight{id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature},
                id = $id, not *entity_tombstone{id}, not *entity_tombstone{id: item_id}
            :limit 1
            "#,
            BTreeMap::from([("id".to_string(), DataValue::from(id))]),
        )?;

        rows.rows
            .first()
            .map(|row| decode_highlight_row(row, 0))
            .transpose()
    }

    /// List highlights for one item, newest first.
    pub fn list_item_highlights(&self, item_id: &str) -> Result<Vec<HighlightRecord>> {
        let rows = self.query_with_params(
            r#"
            ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] :=
                *highlight{id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature},
                item_id = $item_id, not *entity_tombstone{id}, not *entity_tombstone{id: item_id}
            :sort created_at, id
            "#,
            BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
        )?;

        rows.rows
            .iter()
            .map(|row| decode_highlight_row(row, 0))
            .collect()
    }

    /// List highlights with item context, newest first.
    pub fn list_highlights(&self, limit: usize) -> Result<Vec<HighlightWithItem>> {
        let limit = limit.clamp(1, 500);
        let rows = self.query(&format!(
            r#"
            ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature, item_title, item_primary_url] :=
                *highlight{{id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature}},
                *item{{id: item_id, title: item_title, primary_url: item_primary_url}},
                not *entity_tombstone{{id}}, not *entity_tombstone{{id: item_id}}
            :sort created_at, id
            :limit {limit}
            "#
        ))?;

        rows.rows
            .iter()
            .map(|row| {
                Ok(HighlightWithItem {
                    highlight: decode_highlight_row(row, 0)?,
                    item_title: value_as_string(required(row, 9, "item.title")?)?,
                    item_primary_url: optional_string(required(row, 10, "item.primary_url")?)?,
                })
            })
            .collect()
    }

    /// Create a signed note, preserving identity on identical creation retries.
    pub fn put_note(&self, input: PutNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        validate_note_body(&input.body)?;
        if input.target_kind.is_some() != input.target_id.is_some() {
            return Err(StoreError::InvalidInput {
                context: "note.target",
                message: "target kind and target id must be provided together".to_string(),
            });
        }
        validate_note_target(input.target_kind.as_deref(), input.target_id.as_deref())?;

        let created_at_micros = input.created_at_micros.unwrap_or_else(now_micros);
        let created_by = *signer.public();
        let id_payload = NoteIdPayload {
            body: &input.body,
            target_kind: input.target_kind.as_deref(),
            target_id: input.target_id.as_deref(),
        };
        let id_content = canonical_bytes(&id_payload).map_err(|source| StoreError::Signing {
            context: "note id payload",
            message: source.to_string(),
        })?;
        let id = id_for_authored(
            &id_content,
            created_by.as_bytes(),
            created_at_micros.div_euclid(1_000_000),
        );
        let unsigned = UnsignedNote {
            id,
            body: input.body,
            target_kind: input.target_kind,
            target_id: input.target_id,
            created_by,
            created_at_micros,
            updated_at_micros: created_at_micros,
            updated_by: created_by,
            signature_version: 2,
        };
        if let Some(existing) = self.get_note(&unsigned.id)? {
            return Ok(existing);
        }
        let signature = sign_note(&unsigned, signer)?;

        self.query_with_params(
            r#"
            ?[id, body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, signature] :=
                id = $id,
                body = $body,
                target_kind = $target_kind,
                target_id = $target_id,
                created_by = $created_by,
                created_at = $created_at,
                updated_at = $updated_at,
                updated_by = $created_by,
                signature_version = 2,
                signature = $signature
            :insert note {id, updated_at => body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature}
            "#,
            BTreeMap::from([
                ("id".to_string(), DataValue::from(unsigned.id.as_str())),
                ("body".to_string(), DataValue::from(unsigned.body.as_str())),
                (
                    "target_kind".to_string(),
                    option_string(unsigned.target_kind.as_deref()),
                ),
                (
                    "target_id".to_string(),
                    option_string(unsigned.target_id.as_deref()),
                ),
                (
                    "created_by".to_string(),
                    DataValue::Bytes(unsigned.created_by.as_bytes().to_vec()),
                ),
                (
                    "created_at".to_string(),
                    validity_micros(unsigned.created_at_micros),
                ),
                (
                    "updated_at".to_string(),
                    validity_micros(unsigned.updated_at_micros),
                ),
                (
                    "signature".to_string(),
                    DataValue::Bytes(signature.as_bytes().to_vec()),
                ),
            ]),
        )?;

        Ok(NoteRecord {
            id: unsigned.id,
            body: unsigned.body,
            target_kind: unsigned.target_kind,
            target_id: unsigned.target_id,
            created_by: unsigned.created_by,
            created_at_micros: unsigned.created_at_micros,
            updated_at_micros: unsigned.updated_at_micros,
            signature,
            updated_by: unsigned.updated_by,
            signature_version: unsigned.signature_version,
        })
    }

    /// Read the latest known note revision, excluding removed targets.
    pub fn get_note(&self, id: &str) -> Result<Option<NoteRecord>> {
        let rows = self.query_with_params(&format!(r#"
            {NOTE_ALIVE_RULES}
            ?[id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version] :=
                *note{{id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version @ "END"}},
                id = $id, live_note[id]
        "#), BTreeMap::from([("id".into(), DataValue::from(id))]))?;
        rows.rows
            .first()
            .map(|row| decode_note_row(row, 0))
            .transpose()
    }

    /// Append an edit without changing the note's identity or immutable metadata.
    pub fn edit_note(&self, input: EditNoteInput, signer: &Keypair) -> Result<NoteRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        validate_note_body(&input.body)?;
        let previous =
            self.get_note(&input.id)?
                .ok_or_else(|| StoreError::AnnotationUnavailable {
                    id: input.id.clone(),
                })?;
        previous.verify_signature()?;
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(&previous.created_by)
        {
            return Err(StoreError::AnnotationUnauthorized { id: input.id });
        }
        if previous.updated_at_micros != input.expected_updated_at_micros {
            return Err(StoreError::AnnotationConflict { id: input.id });
        }
        let at = next_note_revision(
            input.now_micros.unwrap_or_else(now_micros),
            previous.updated_at_micros,
        )
        .ok_or_else(|| StoreError::InvalidInput {
            context: "note revision",
            message: "revision timestamp overflow".into(),
        })?;
        let unsigned = UnsignedNote {
            id: previous.id,
            body: input.body,
            target_kind: previous.target_kind,
            target_id: previous.target_id,
            created_by: previous.created_by,
            created_at_micros: previous.created_at_micros,
            updated_at_micros: at,
            updated_by: *signer.public(),
            signature_version: 2,
        };
        let signature = sign_note(&unsigned, signer)?;
        let rows = self.query_with_params(&format!(r#"
            {NOTE_ALIVE_RULES}
            ?[id, updated_at, body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature] :=
                *note{{id, updated_at: previous @ "END"}}, id = $id,
                to_int(previous) == $expected, live_note[id],
                updated_at = $at, body = $body, target_kind = $kind, target_id = $target,
                created_by = $author, created_at = $created, updated_by = $updater,
                signature_version = 2, signature = $signature
            :insert note {{id, updated_at => body, target_kind, target_id, created_by, created_at, updated_by, signature_version, signature}}
            :returning
        "#), BTreeMap::from([
            ("id".into(), DataValue::from(unsigned.id.as_str())), ("expected".into(), DataValue::from(input.expected_updated_at_micros)),
            ("at".into(), validity_micros(at)), ("body".into(), DataValue::from(unsigned.body.as_str())),
            ("kind".into(), option_string(unsigned.target_kind.as_deref())), ("target".into(), option_string(unsigned.target_id.as_deref())),
            ("author".into(), DataValue::Bytes(unsigned.created_by.as_bytes().to_vec())), ("created".into(), validity_micros(unsigned.created_at_micros)),
            ("updater".into(), DataValue::Bytes(unsigned.updated_by.as_bytes().to_vec())), ("signature".into(), DataValue::Bytes(signature.as_bytes().to_vec())),
        ]))?;
        if rows.rows.is_empty() {
            return Err(StoreError::AnnotationConflict { id: unsigned.id });
        }
        Ok(NoteRecord {
            id: unsigned.id,
            body: unsigned.body,
            target_kind: unsigned.target_kind,
            target_id: unsigned.target_id,
            created_by: unsigned.created_by,
            created_at_micros: unsigned.created_at_micros,
            updated_at_micros: at,
            updated_by: unsigned.updated_by,
            signature_version: 2,
            signature,
        })
    }

    /// Return the latest signed removal of an annotation, if one exists.
    pub fn get_annotation_removal(&self, id: &str) -> Result<Option<AnnotationRemovalRecord>> {
        let rows = self.query_with_params(
            r#"
            ?[id, kind, removed_by, removed_at, signature] :=
                *annotation_removal{id, kind, removed_by, removed_at, signature @ "END"}, id = $id
        "#,
            BTreeMap::from([("id".into(), DataValue::from(id))]),
        )?;
        rows.rows
            .first()
            .map(|row| {
                Ok(AnnotationRemovalRecord {
                    id: value_as_string(required(row, 0, "removal.id")?)?,
                    kind: value_as_string(required(row, 1, "removal.kind")?)?,
                    removed_by: value_as_public_key(required(row, 2, "removal.author")?)?,
                    removed_at_micros: value_as_validity_micros(required(row, 3, "removal.at")?)?,
                    signature: value_as_signature(required(row, 4, "removal.signature")?)?,
                })
            })
            .transpose()
    }

    /// Permanently hide an owned highlight and its notes, retaining signed history.
    pub fn remove_highlight(&self, id: &str, signer: &Keypair) -> Result<AnnotationRemovalRecord> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        let permissions = self.annotation_creator_permissions_unlocked(signer.public())?;
        if let Some(existing) = self.get_annotation_removal(id)? {
            existing.verify_signature()?;
            if !permissions.contains(&existing.removed_by) {
                return Err(StoreError::AnnotationUnauthorized { id: id.into() });
            }
            return Ok(existing);
        }
        let record = self
            .get_highlight(id)?
            .ok_or_else(|| StoreError::AnnotationUnavailable { id: id.into() })?;
        record.verify_signature()?;
        if !permissions.contains(&record.created_by) {
            return Err(StoreError::AnnotationUnauthorized { id: id.into() });
        }
        let mut removal = AnnotationRemovalRecord {
            id: id.into(),
            kind: "highlight".into(),
            removed_by: *signer.public(),
            removed_at_micros: now_micros(),
            signature: record.signature,
        };
        removal.signature =
            sign_canonical(signer, &removal.payload()).map_err(|error| StoreError::Signing {
                context: "annotation removal",
                message: error.to_string(),
            })?;
        self.query_with_params(r#"
            { ?[id, removed_at, kind, removed_by, signature] <- [[$id, $at, 'highlight', $author, $signature]]
              :insert annotation_removal {id, removed_at => kind, removed_by, signature} }
            { ?[id, tombstoned_at, reason] <- [[$id, $at, 'removed highlight']]
              :put entity_tombstone {id, tombstoned_at => reason} }
        "#, BTreeMap::from([
            ("id".into(), DataValue::from(id)), ("at".into(), validity_micros(removal.removed_at_micros)),
            ("author".into(), DataValue::Bytes(removal.removed_by.as_bytes().to_vec())), ("signature".into(), DataValue::Bytes(removal.signature.as_bytes().to_vec())),
        ]))?;
        Ok(removal)
    }

    /// List notes for a target, newest first.
    pub fn list_notes_for_target(
        &self,
        target_kind: &str,
        target_id: &str,
    ) -> Result<Vec<NoteRecord>> {
        let rows = self.query_with_params(
            &format!(r#"
            {NOTE_ALIVE_RULES}
            ?[id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version] :=
                *note{{id, body, target_kind, target_id, created_by, created_at, updated_at, signature, updated_by, signature_version @ "END"}},
                target_kind = $target_kind,
                target_id = $target_id, live_note[id]
            :sort updated_at, id
            "#),
            BTreeMap::from([
                ("target_kind".to_string(), DataValue::from(target_kind)),
                ("target_id".to_string(), DataValue::from(target_id)),
            ]),
        )?;

        rows.rows
            .iter()
            .map(|row| decode_note_row(row, 0))
            .collect()
    }
}

struct UnsignedHighlight {
    id: String,
    item_id: String,
    quoted_text: String,
    location: HighlightLocation,
    location_label: Option<String>,
    color: Option<String>,
    created_by: PublicKey,
    created_at_micros: i64,
}

struct UnsignedNote {
    id: String,
    body: String,
    target_kind: Option<String>,
    target_id: Option<String>,
    created_by: PublicKey,
    created_at_micros: i64,
    updated_at_micros: i64,
    updated_by: PublicKey,
    signature_version: i64,
}

#[derive(Serialize)]
struct HighlightIdPayload<'a> {
    item_id: &'a str,
    quoted_text: &'a str,
    location: &'a HighlightLocation,
}

#[derive(Serialize)]
struct HighlightSignaturePayload<'a> {
    relation: &'static str,
    id: &'a str,
    item_id: &'a str,
    quoted_text: &'a str,
    location: &'a HighlightLocation,
    location_label: Option<&'a str>,
    color: Option<&'a str>,
    created_by: PublicKey,
    created_at_micros: i64,
}

#[derive(Serialize)]
struct NoteIdPayload<'a> {
    body: &'a str,
    target_kind: Option<&'a str>,
    target_id: Option<&'a str>,
}

#[derive(Serialize)]
struct NoteSignaturePayload<'a> {
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

fn sign_highlight(unsigned: &UnsignedHighlight, signer: &Keypair) -> Result<Signature> {
    sign_canonical(signer, &highlight_signature_payload(unsigned)).map_err(|source| {
        StoreError::Signing {
            context: "highlight signature payload",
            message: source.to_string(),
        }
    })
}

fn sign_note(unsigned: &UnsignedNote, signer: &Keypair) -> Result<Signature> {
    sign_canonical(signer, &note_signature_payload(unsigned)).map_err(|source| {
        StoreError::Signing {
            context: "note signature payload",
            message: source.to_string(),
        }
    })
}

fn highlight_signature_payload(unsigned: &UnsignedHighlight) -> HighlightSignaturePayload<'_> {
    // The signature covers exactly the persisted immutable highlight columns.
    // Sync verification can rebuild this payload from the row and device key.
    HighlightSignaturePayload {
        relation: "highlight",
        id: &unsigned.id,
        item_id: &unsigned.item_id,
        quoted_text: &unsigned.quoted_text,
        location: &unsigned.location,
        location_label: unsigned.location_label.as_deref(),
        color: unsigned.color.as_deref(),
        created_by: unsigned.created_by,
        created_at_micros: unsigned.created_at_micros,
    }
}

fn highlight_signature_payload_from_record(
    record: &HighlightRecord,
) -> HighlightSignaturePayload<'_> {
    // This mirrors `highlight_signature_payload` for tests and future inbound
    // fact verification.
    HighlightSignaturePayload {
        relation: "highlight",
        id: &record.id,
        item_id: &record.item_id,
        quoted_text: &record.quoted_text,
        location: &record.location,
        location_label: record.location_label.as_deref(),
        color: record.color.as_deref(),
        created_by: record.created_by,
        created_at_micros: record.created_at_micros,
    }
}

fn note_signature_payload(unsigned: &UnsignedNote) -> NoteSignaturePayload<'_> {
    NoteSignaturePayload {
        relation: "note",
        id: &unsigned.id,
        body: &unsigned.body,
        target_kind: unsigned.target_kind.as_deref(),
        target_id: unsigned.target_id.as_deref(),
        created_by: unsigned.created_by,
        created_at_micros: unsigned.created_at_micros,
        updated_at_micros: unsigned.updated_at_micros,
        updated_by: (unsigned.signature_version == 2).then_some(unsigned.updated_by),
        signature_version: (unsigned.signature_version == 2).then_some(2),
    }
}

fn note_signature_payload_from_record(record: &NoteRecord) -> NoteSignaturePayload<'_> {
    NoteSignaturePayload {
        relation: "note",
        id: &record.id,
        body: &record.body,
        target_kind: record.target_kind.as_deref(),
        target_id: record.target_id.as_deref(),
        created_by: record.created_by,
        created_at_micros: record.created_at_micros,
        updated_at_micros: record.updated_at_micros,
        updated_by: (record.signature_version == 2).then_some(record.updated_by),
        signature_version: (record.signature_version == 2).then_some(2),
    }
}

fn decode_highlight_row(row: &[DataValue], offset: usize) -> Result<HighlightRecord> {
    Ok(HighlightRecord {
        id: value_as_string(required(row, offset, "highlight.id")?)?,
        item_id: value_as_string(required(row, offset + 1, "highlight.item_id")?)?,
        quoted_text: value_as_string(required(row, offset + 2, "highlight.quoted_text")?)?,
        location: value_as_location(required(row, offset + 3, "highlight.location")?)?,
        location_label: optional_string(required(row, offset + 4, "highlight.location_label")?)?,
        color: optional_string(required(row, offset + 5, "highlight.color")?)?,
        created_by: value_as_public_key(required(row, offset + 6, "highlight.created_by")?)?,
        created_at_micros: value_as_validity_micros(required(
            row,
            offset + 7,
            "highlight.created_at",
        )?)?,
        signature: value_as_signature(required(row, offset + 8, "highlight.signature")?)?,
    })
}

fn decode_note_row(row: &[DataValue], offset: usize) -> Result<NoteRecord> {
    Ok(NoteRecord {
        id: value_as_string(required(row, offset, "note.id")?)?,
        body: value_as_string(required(row, offset + 1, "note.body")?)?,
        target_kind: optional_string(required(row, offset + 2, "note.target_kind")?)?,
        target_id: optional_string(required(row, offset + 3, "note.target_id")?)?,
        created_by: value_as_public_key(required(row, offset + 4, "note.created_by")?)?,
        created_at_micros: value_as_validity_micros(required(row, offset + 5, "note.created_at")?)?,
        updated_at_micros: value_as_validity_micros(required(row, offset + 6, "note.updated_at")?)?,
        signature: value_as_signature(required(row, offset + 7, "note.signature")?)?,
        updated_by: value_as_public_key(required(row, offset + 8, "note.updated_by")?)?,
        signature_version: match required(row, offset + 9, "note.signature_version")? {
            DataValue::Num(cozo::Num::Int(version)) => *version,
            other => return Err(unexpected("note.signature_version", other)),
        },
    })
}

fn required<'a>(
    row: &'a [DataValue],
    index: usize,
    context: &'static str,
) -> Result<&'a DataValue> {
    row.get(index).ok_or_else(|| StoreError::UnexpectedValue {
        context,
        value: "missing column".to_string(),
    })
}

fn value_as_string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        other => Err(unexpected("string", other)),
    }
}

fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Str(value) => Ok(Some(value.to_string())),
        other => Err(unexpected("optional string", other)),
    }
}

fn value_as_validity_micros(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Validity(value) => Ok(value.timestamp.0.0),
        other => Err(unexpected("validity", other)),
    }
}

fn value_as_location(value: &DataValue) -> Result<HighlightLocation> {
    match value {
        DataValue::Json(JsonData(value)) => {
            serde_json::from_value(value.clone()).map_err(|source| StoreError::UnexpectedValue {
                context: "highlight.location",
                value: source.to_string(),
            })
        }
        other => Err(unexpected("highlight location JSON", other)),
    }
}

fn value_as_public_key(value: &DataValue) -> Result<PublicKey> {
    match value {
        DataValue::Bytes(value) => {
            PublicKey::from_slice(value).map_err(|source| StoreError::UnexpectedValue {
                context: "public key bytes",
                value: source.to_string(),
            })
        }
        other => Err(unexpected("public key bytes", other)),
    }
}

fn value_as_signature(value: &DataValue) -> Result<Signature> {
    match value {
        DataValue::Bytes(value) => {
            Signature::from_slice(value).map_err(|source| StoreError::UnexpectedValue {
                context: "signature bytes",
                value: source.to_string(),
            })
        }
        other => Err(unexpected("signature bytes", other)),
    }
}

fn option_string(value: Option<&str>) -> DataValue {
    value.map(DataValue::from).unwrap_or(DataValue::Null)
}

fn location_json(location: &HighlightLocation) -> Result<serde_json::Value> {
    serde_json::to_value(location).map_err(|source| StoreError::UnexpectedValue {
        context: "highlight.location",
        value: source.to_string(),
    })
}

fn validity_micros(micros: i64) -> DataValue {
    DataValue::Validity(cozo::Validity::from((micros, true)))
}

fn now_micros() -> i64 {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000).unwrap_or(i64::MAX)
}

fn unexpected(context: &'static str, value: &DataValue) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: format!("{value:?}"),
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
    use carrel_core::identity::verify_canonical;
    use cozo::{Num, Validity};

    use super::*;

    const FIXED_TIME: i64 = 1_700_000_123_456_789;

    #[test]
    fn highlight_ids_and_signatures_are_deterministic() {
        let store = migrated_store();
        let signer = Keypair::from_seed(&[7_u8; 32]);
        insert_item(&store, "item-1", "A Quiet Web");

        let first = store
            .put_highlight(highlight_input("item-1", FIXED_TIME), &signer)
            .unwrap();
        let second = store
            .put_highlight(highlight_input("item-1", FIXED_TIME), &signer)
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(first.signature, second.signature);
        verify_canonical(
            signer.public(),
            &highlight_signature_payload_from_record(&first),
            &first.signature,
        )
        .unwrap();
    }

    #[test]
    fn highlight_round_trips_and_lists_with_item_context() {
        let store = migrated_store();
        let signer = Keypair::from_seed(&[8_u8; 32]);
        insert_item(&store, "item-1", "A Quiet Web");
        insert_item(&store, "item-2", "Elsewhere");

        let newer = store
            .put_highlight(highlight_input("item-1", FIXED_TIME + 1_000_000), &signer)
            .unwrap();
        let older = store
            .put_highlight(highlight_input("item-2", FIXED_TIME), &signer)
            .unwrap();

        assert_eq!(store.get_highlight(&newer.id).unwrap(), Some(newer.clone()));
        assert_eq!(
            store.list_item_highlights("item-1").unwrap(),
            vec![newer.clone()]
        );

        let all = store.list_highlights(10).unwrap();
        assert_eq!(all[0].highlight.id, newer.id);
        assert_eq!(all[0].item_title, "A Quiet Web");
        assert_eq!(all[1].highlight.id, older.id);
        assert_eq!(
            all[1].item_primary_url.as_deref(),
            Some("https://example.com/Elsewhere")
        );
    }

    #[test]
    fn note_round_trips_for_highlight_target() {
        let store = migrated_store();
        let signer = Keypair::from_seed(&[9_u8; 32]);
        insert_item(&store, "item-1", "A Quiet Web");
        let highlight = store
            .put_highlight(highlight_input("item-1", FIXED_TIME), &signer)
            .unwrap();

        let note = store
            .put_note(
                PutNoteInput {
                    body: "This is worth returning to.".to_string(),
                    target_kind: Some("highlight".to_string()),
                    target_id: Some(highlight.id.clone()),
                    created_at_micros: Some(FIXED_TIME + 2_000_000),
                },
                &signer,
            )
            .unwrap();

        assert_eq!(
            store
                .list_notes_for_target("highlight", &highlight.id)
                .unwrap(),
            vec![note.clone()]
        );
        verify_canonical(
            signer.public(),
            &note_signature_payload_from_record(&note),
            &note.signature,
        )
        .unwrap();
    }

    #[test]
    fn validation_rejects_bad_selector_and_note_target_pairs() {
        let store = migrated_store();
        let signer = Keypair::from_seed(&[10_u8; 32]);

        let mut input = highlight_input("item-1", FIXED_TIME);
        input.location = HighlightLocation::HtmlText(HtmlTextSelector {
            content_blob_id: None,
            exact: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            start: 0,
            end: 1,
        });
        assert!(matches!(
            store.put_highlight(input, &signer),
            Err(StoreError::InvalidInput {
                context: "highlight.location",
                ..
            })
        ));

        assert!(matches!(
            store.put_note(
                PutNoteInput {
                    body: "   ".to_string(),
                    target_kind: Some("highlight".to_string()),
                    target_id: Some("highlight-1".to_string()),
                    created_at_micros: Some(FIXED_TIME),
                },
                &signer,
            ),
            Err(StoreError::InvalidInput {
                context: "note.body",
                ..
            })
        ));
        assert!(matches!(
            store.put_note(
                PutNoteInput {
                    body: "body".to_string(),
                    target_kind: Some("highlight".to_string()),
                    target_id: None,
                    created_at_micros: Some(FIXED_TIME),
                },
                &signer,
            ),
            Err(StoreError::InvalidInput {
                context: "note.target",
                ..
            })
        ));
    }

    fn migrated_store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
    }

    fn highlight_input(item_id: &str, created_at_micros: i64) -> PutHighlightInput {
        PutHighlightInput {
            item_id: item_id.to_string(),
            location: HighlightLocation::HtmlText(HtmlTextSelector {
                content_blob_id: Some("content-1".to_string()),
                exact: "quiet web".to_string(),
                prefix: "the".to_string(),
                suffix: "matters".to_string(),
                start: 4,
                end: 13,
            }),
            location_label: Some("p. 1".to_string()),
            color: Some("yellow".to_string()),
            created_at_micros: Some(created_at_micros),
        }
    }

    fn insert_item(store: &Store, id: &str, title: &str) {
        store
            .query_with_params(
                r#"
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = $id,
                    kind = 'article',
                    title = $title,
                    creators = [],
                    primary_url = $primary_url,
                    published_at = null,
                    language = 'en',
                    summary = null,
                    discovered_at = $discovered_at
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
                "#,
                BTreeMap::from([
                    ("id".to_string(), DataValue::from(id)),
                    ("title".to_string(), DataValue::from(title)),
                    (
                        "primary_url".to_string(),
                        DataValue::from(format!("https://example.com/{title}").as_str()),
                    ),
                    (
                        "discovered_at".to_string(),
                        DataValue::Validity(Validity::from((FIXED_TIME, true))),
                    ),
                ]),
            )
            .unwrap();

        store
            .query_with_params(
                r#"
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = $item_id,
                    feed_url = null,
                    word_count = $word_count,
                    estimated_read_minutes = 3,
                    site_name = 'Example',
                    byline = null
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(id)),
                    ("word_count".to_string(), DataValue::Num(Num::Int(600))),
                ]),
            )
            .unwrap();
    }
}
