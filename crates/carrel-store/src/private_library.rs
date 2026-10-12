//! Signed saved-article history, durable mirror work, and source adoption.

use std::collections::{BTreeMap, BTreeSet};

use carrel_core::identity::{Keypair, PublicKey, RevocationCert, Signature};
use carrel_core::private_library::{
    ArticleMetadata, BlobReference, ContentReference, ItemIdentifier, ItemRevision, PrivateFact,
    PrivateHighlight, PrivateRemoval, SavedItem, resolve_item_heads,
};
use cozo::{DataValue, JsonData, Validity};

use crate::identity::bytes;
use crate::{Result, Store, StoreError};

pub(crate) const QUEUE_PRIVATE_FACT: &str = r#"
    { ?[key, encoded] <- [[$pf_key, $pf_encoded]] :put self_fact_history {key => encoded} }
    { ?[key, encoded] <- [[$pf_key, $pf_encoded]] :put self_fact_outbox {key => encoded} }
"#;

const QUEUE_BLOBS: &str =
    "{ ?[blob_id, byte_size] <- $pf_blobs :put self_blob_incoming {blob_id => byte_size} }";

const HEAD_RULES: &str = r#"
    superseded[id, parent] := *self_item_revision{id, parents}, parent in parents
    head[id, rev] := *self_item_revision{id, revision_id: rev}, not superseded[id, rev]
"#;

/// Source metadata captured under the feed/write lock, before reading cache files.
#[derive(Clone, Debug)]
pub struct ItemCapture {
    /// Original metadata; the caller adds verified cached-resource references.
    pub item: SavedItem,
    /// Opaque source generation checked again before signing.
    pub token: String,
}

/// A source refresh cannot acknowledge a newer edit made during file extraction.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemAdoption {
    /// Signed history and its mirror work were committed atomically.
    Recorded(Box<ItemRevision>),
    /// The source is identical to its current signed projection.
    Unchanged,
    /// A newer source generation remains dirty and must be captured again.
    Stale,
}

/// Result of a verified item delivery from an authenticated self device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemReceive {
    /// History, projection, and the forwarding queue changed atomically.
    Applied,
    /// Identical immutable history already exists.
    Existing,
    /// Durable staging waits for parents or unpublished local source changes.
    Pending,
    /// Permanent removal dominates the incoming revision.
    Removed,
}

/// Common result for the bridge's explicit allow-list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateReceive {
    /// Verified immutable history and its projection changed.
    Applied,
    /// Exact immutable history was already present.
    Existing,
    /// Verified bytes wait durably for dependencies.
    Pending,
    /// Permanent removal dominates delivery.
    Removed,
}

impl Store {
    /// Check a portable record against the chosen Root before exposing its bytes.
    /// Historical certified authors remain valid; this installation must be active.
    pub fn private_fact_authorized(&self, fact: &PrivateFact) -> Result<bool> {
        let _guard = self.identity_lock.lock().expect("identity read lock");
        fact.verify()?;
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        let known: BTreeSet<_> = self
            .self_devices()?
            .into_iter()
            .map(|row| row.certificate.device_pubkey)
            .collect();
        Ok(match fact {
            PrivateFact::Item(record) => {
                known.contains(&record.author) && record.item.kind == "article"
            }
            PrivateFact::State(record) => known.contains(&record.author),
            PrivateFact::Note(record) => {
                known.contains(&record.created_by) && known.contains(&record.updated_by)
            }
            PrivateFact::Highlight(record) => known.contains(&record.created_by),
            PrivateFact::Removal(record) => known.contains(&record.removed_by),
            PrivateFact::Certificate(record) => record.master_pubkey == binding.master_pubkey,
            PrivateFact::Revocation(record) => record.master_pubkey == binding.master_pubkey,
        })
    }
    /// Adopt already signed legacy facts without changing their original proof.
    ///
    /// Run once after explicit identity setup or on recovery. Source reads are
    /// paged; future typed writes already journal portable history atomically.
    /// Records outside this Root remain local and are never queued for self-sync.
    pub fn seed_existing_private_facts(&self) -> Result<usize> {
        let _private_guard = self
            .private_state_lock
            .lock()
            .expect("private library write lock");
        let _annotation_guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        let known: BTreeSet<_> = self
            .self_devices()?
            .into_iter()
            .map(|device| device.certificate.device_pubkey)
            .collect();
        let mut seeded = 0;
        for source in 0..4 {
            let mut after_id = DataValue::from("");
            let mut after_second = if source == 2 {
                DataValue::from(i64::MIN)
            } else {
                DataValue::from("")
            };
            let mut first = true;
            loop {
                let script = match source {
                    0 => format!("?[{}] := *note{{{}}}, ($first || id > $after_id || (id == $after_id && revision_id > $after_second)) :sort id, revision_id :limit 100", crate::notes::NOTE_COLUMNS, crate::notes::NOTE_COLUMNS),
                    1 => "?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] := *highlight{id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature}, ($first || id > $after_id) :sort id :limit 100".into(),
                    2 => "?[id, kind, removed_by, at, signature] := *annotation_removal{id, kind, removed_by, removed_at, signature}, at = to_int(removed_at), ($first || id > $after_id || (id == $after_id && at > $after_second)) :sort id, at :limit 100".into(),
                    _ => "?[register, revision_id, encoded] := *self_state{register, revision_id, encoded}, ($first || register > $after_id || (register == $after_id && revision_id > $after_second)) :sort register, revision_id :limit 100".into(),
                };
                let rows = self.query_with_params(
                    &script,
                    BTreeMap::from([
                        ("after_id".into(), after_id.clone()),
                        ("after_second".into(), after_second.clone()),
                        ("first".into(), DataValue::Bool(first)),
                    ]),
                )?;
                if rows.rows.is_empty() {
                    break;
                }
                for row in &rows.rows {
                    let fact = match source {
                        0 => PrivateFact::Note(
                            crate::notes::decode_note_row(row, 0)?.into_portable(),
                        ),
                        1 => PrivateFact::Highlight(PrivateHighlight::from(
                            &crate::highlights::decode_highlight_row(row, 0)?,
                        )),
                        2 => PrivateFact::Removal(PrivateRemoval {
                            id: string(&row[0])?,
                            kind: string(&row[1])?,
                            removed_by: public_key(&row[2])?,
                            removed_at_micros: int(&row[3])?,
                            signature: signature(&row[4])?,
                        }),
                        _ => PrivateFact::State(
                            carrel_core::private_state::StateRevision::from_bytes(bytes(&row[2])?)?,
                        ),
                    };
                    let authorized = match &fact {
                        PrivateFact::Note(note) => {
                            known.contains(&note.created_by) && known.contains(&note.updated_by)
                        }
                        PrivateFact::Highlight(highlight) => known.contains(&highlight.created_by),
                        PrivateFact::Removal(removal) => known.contains(&removal.removed_by),
                        PrivateFact::State(state) => known.contains(&state.author),
                        _ => false,
                    };
                    if authorized && self.seed_private_fact(&fact)? {
                        seeded += 1;
                    }
                }
                let last = rows.rows.last().expect("nonempty source page");
                after_id = last[0].clone();
                after_second = match source {
                    0 => last[10].clone(),
                    2 => last[3].clone(),
                    _ => last[1].clone(),
                };
                first = false;
            }
        }
        for device in self.self_devices()? {
            if self.seed_private_fact(&PrivateFact::Certificate(device.certificate))? {
                seeded += 1;
            }
        }
        // A proof can precede its device certificate, so reading only the device
        // list would lose a permanent revocation during mirror recovery.
        let rows = self.query_with_params(
            "?[encoded] := *device_revocation{master_pubkey: $master, revocation_cert: encoded}",
            BTreeMap::from([(
                "master".into(),
                DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
            )]),
        )?;
        for row in rows.rows {
            if self.seed_private_fact(&PrivateFact::Revocation(RevocationCert::from_bytes(
                bytes(&row[0])?,
            )?))? {
                seeded += 1;
            }
        }
        Ok(seeded)
    }

    fn seed_private_fact(&self, fact: &PrivateFact) -> Result<bool> {
        let params = private_fact_params(fact)?;
        if !self
            .query_with_params(
                "?[key] := *self_fact_history{key}, key = $pf_key",
                params.clone(),
            )?
            .rows
            .is_empty()
        {
            return Ok(false);
        }
        self.query_with_params(QUEUE_PRIVATE_FACT, params)?;
        Ok(true)
    }
    /// Accept only a verified fact type on the authenticated private channel.
    pub fn accept_self_private_fact(
        &self,
        fact: &PrivateFact,
        sender: &PublicKey,
    ) -> Result<PrivateReceive> {
        fact.verify()?;
        // Each handler orders authorization against its write transaction.
        match fact {
            PrivateFact::Item(record) => {
                Ok(match self.accept_self_item_revision(record, sender)? {
                    ItemReceive::Applied => PrivateReceive::Applied,
                    ItemReceive::Existing => PrivateReceive::Existing,
                    ItemReceive::Pending => PrivateReceive::Pending,
                    ItemReceive::Removed => PrivateReceive::Removed,
                })
            }
            PrivateFact::State(record) => {
                Ok(match self.accept_self_state_revision(record, sender)? {
                    crate::private_state::StateReceive::Applied => PrivateReceive::Applied,
                    crate::private_state::StateReceive::Existing => PrivateReceive::Existing,
                    crate::private_state::StateReceive::Pending => PrivateReceive::Pending,
                    crate::private_state::StateReceive::Removed => PrivateReceive::Removed,
                })
            }
            PrivateFact::Note(record) => {
                Ok(match self.accept_self_note_revision(record, sender)? {
                    crate::notes::NoteReceive::Applied => PrivateReceive::Applied,
                    crate::notes::NoteReceive::Existing => PrivateReceive::Existing,
                    crate::notes::NoteReceive::Pending => PrivateReceive::Pending,
                    crate::notes::NoteReceive::Removed => PrivateReceive::Removed,
                })
            }
            PrivateFact::Highlight(record) => self.accept_self_highlight(record, sender),
            PrivateFact::Removal(record) => self.accept_self_annotation_removal(record, sender),
            PrivateFact::Certificate(record) => {
                self.accept_self_certificate_from(record, sender)?;
                Ok(PrivateReceive::Applied)
            }
            PrivateFact::Revocation(record) => {
                self.accept_self_revocation_from(record, sender)?;
                Ok(PrivateReceive::Applied)
            }
        }
    }

    /// Verify an immutable highlight, retaining its original author and signature.
    pub fn accept_self_highlight(
        &self,
        record: &PrivateHighlight,
        sender: &PublicKey,
    ) -> Result<PrivateReceive> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        record.verify_signature()?;
        self.require_private_sender(sender, Some(&record.created_by))?;
        if self.private_item_removed(&record.id)? || self.private_item_removed(&record.item_id)? {
            self.discard_library_pending(&PrivateFact::Highlight(record.clone()).key()?)?;
            return Ok(PrivateReceive::Removed);
        }
        if let Some(known) = self.get_highlight(&record.id)? {
            if PrivateHighlight::from(&known) != *record {
                return Err(invalid("immutable highlight collision"));
            }
            return Ok(PrivateReceive::Existing);
        }
        if self.get_item_detail(&record.item_id)?.is_none() {
            self.stage_library_fact(
                &PrivateFact::Highlight(record.clone()),
                Some(&record.item_id),
            )?;
            return Ok(PrivateReceive::Pending);
        }
        self.write_private_highlight(record)?;
        Ok(PrivateReceive::Applied)
    }

    /// Signed removal is permanent even if it precedes its annotation or target.
    pub fn accept_self_annotation_removal(
        &self,
        record: &PrivateRemoval,
        sender: &PublicKey,
    ) -> Result<PrivateReceive> {
        let _guard = self.annotation_lock.lock().expect("annotation write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        record.verify_signature()?;
        self.require_private_sender(sender, Some(&record.removed_by))?;
        let fact = PrivateFact::Removal(record.clone());
        let key = fact.key()?;
        if !self
            .query_with_params(
                "?[key] := *self_fact_history{key}, key = $key",
                BTreeMap::from([("key".into(), DataValue::from(key.as_str()))]),
            )?
            .rows
            .is_empty()
        {
            return Ok(PrivateReceive::Existing);
        }
        // Different concurrent valid proofs may share an ID/time; the wire
        // history retains each proof, while a permanent tombstone suffices for
        // the query projection. Never replace an already stored signed proof.
        let known = self.query_with_params("?[kind, removed_by, signature] := *annotation_removal{id: $id, removed_at: $at, kind, removed_by, signature}", BTreeMap::from([
            ("id".into(), DataValue::from(record.id.as_str())), ("at".into(), validity(record.removed_at_micros)),
        ]))?;
        if let Some(row) = known.rows.first() {
            if string(&row[0])? == record.kind
                && bytes(&row[1])? == record.removed_by.as_bytes()
                && bytes(&row[2])? == record.signature.as_bytes()
            {
                self.discard_library_pending(&key)?;
                return Ok(PrivateReceive::Existing);
            }
            // Existing signatures cannot be overwritten. Keep this distinct
            // proof in immutable mirror bytes and preserve removal dominance.
            let mut params = private_fact_params(&fact)?;
            params.extend(id_params(&record.id));
            params.insert("at".into(), validity(record.removed_at_micros));
            self.query_with_params(&format!(r#"{QUEUE_PRIVATE_FACT}
                {{ ?[id, tombstoned_at, reason] <- [[$id, $at, 'removed annotation']] :put entity_tombstone {{id, tombstoned_at => reason}} }}"#), params)?;
            return Ok(PrivateReceive::Applied);
        }
        self.write_private_removal(record)?;
        Ok(PrivateReceive::Applied)
    }

    /// Promote ready highlights before limiting, including removed targets.
    pub fn promote_pending_highlights(&self) -> Result<Vec<String>> {
        let rows = self.query(r#"
            ready[key] := *self_library_incoming{key, kind: 'highlight', target}, *item{id: target}
            ready[key] := *self_library_incoming{key, kind: 'highlight', target}, *entity_tombstone{id: target @ "END"}
            ?[encoded] := ready[key], *self_library_incoming{key, encoded} :limit 100
        "#)?;
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        let mut applied = Vec::new();
        for row in rows.rows {
            let fact = PrivateFact::from_bytes(bytes(&row[0])?)?;
            let PrivateFact::Highlight(record) = fact else {
                return Err(invalid("invalid staged highlight kind"));
            };
            if self.accept_self_highlight(&record, &binding.device_pubkey)?
                == PrivateReceive::Applied
            {
                applied.push(record.id);
            }
        }
        Ok(applied)
    }

    pub(crate) fn write_private_highlight(&self, record: &PrivateHighlight) -> Result<()> {
        let fact = PrivateFact::Highlight(record.clone());
        let mut params = private_fact_params(&fact)?;
        params.extend([
            ("id".into(), DataValue::from(record.id.as_str())),
            ("item_id".into(), DataValue::from(record.item_id.as_str())),
            (
                "quoted_text".into(),
                DataValue::from(record.quoted_text.as_str()),
            ),
            (
                "location".into(),
                DataValue::Json(JsonData(
                    serde_json::to_value(&record.location)
                        .map_err(|_| invalid("invalid selector encoding"))?,
                )),
            ),
            (
                "location_label".into(),
                optional_string(record.location_label.as_deref()),
            ),
            ("color".into(), optional_string(record.color.as_deref())),
            (
                "author".into(),
                DataValue::Bytes(record.created_by.as_bytes().to_vec()),
            ),
            ("at".into(), validity(record.created_at_micros)),
            (
                "signature".into(),
                DataValue::Bytes(record.signature.as_bytes().to_vec()),
            ),
        ]);
        self.query_with_params(&format!(r#"
            {{ ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] <- [[$id, $item_id, $quoted_text, $location, $location_label, $color, $author, $at, $signature]]
               :put highlight {{id => item_id, quoted_text, location, location_label, color, created_by, created_at, signature}} }}
            {QUEUE_PRIVATE_FACT}
            {{ ?[key] <- [[$pf_key]] :rm self_library_incoming {{key}} }}
        "#), params)?;
        Ok(())
    }

    pub(crate) fn write_private_removal(&self, record: &PrivateRemoval) -> Result<()> {
        let mut params = private_fact_params(&PrivateFact::Removal(record.clone()))?;
        params.extend([
            ("id".into(), DataValue::from(record.id.as_str())),
            ("kind".into(), DataValue::from(record.kind.as_str())),
            (
                "author".into(),
                DataValue::Bytes(record.removed_by.as_bytes().to_vec()),
            ),
            ("at".into(), validity(record.removed_at_micros)),
            (
                "signature".into(),
                DataValue::Bytes(record.signature.as_bytes().to_vec()),
            ),
        ]);
        self.query_with_params(&format!(r#"
            {{ ?[id, removed_at, kind, removed_by, signature] <- [[$id, $at, $kind, $author, $signature]] :insert annotation_removal {{id, removed_at => kind, removed_by, signature}} }}
            {{ ?[id, tombstoned_at, reason] <- [[$id, $at, 'removed annotation']] :put entity_tombstone {{id, tombstoned_at => reason}} }}
            {QUEUE_PRIVATE_FACT}
            {{ ?[key] <- [[$pf_key]] :rm self_library_incoming {{key}} }}
        "#), params)?;
        Ok(())
    }
    /// Page unpublished source IDs; a bad/missing cache must not starve later IDs.
    pub fn dirty_private_items(&self, after: Option<&str>, limit: usize) -> Result<Vec<String>> {
        let rows = self.query_with_params(
            r#"
            removed[id] := *entity_tombstone{id @ "END"}
            ?[id] := *self_item_dirty{id}, *item{id}, not removed[id], ($first || id > $after)
            :sort id :limit $limit
        "#,
            BTreeMap::from([
                ("after".into(), DataValue::from(after.unwrap_or(""))),
                ("first".into(), DataValue::Bool(after.is_none())),
                ("limit".into(), DataValue::from(limit.min(1000) as i64)),
            ]),
        )?;
        rows.rows.iter().map(|row| string(&row[0])).collect()
    }

    /// Capture one unpublished article consistently without doing any file I/O.
    pub fn capture_private_item(&self, id: &str) -> Result<Option<ItemCapture>> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private library write lock");
        let Some(token) = self.item_dirty_token(id)? else {
            return Ok(None);
        };
        let item = self.saved_item_snapshot_unlocked(id)?;
        if self.item_dirty_token(id)?.as_ref() != Some(&token) {
            return Ok(None);
        }
        Ok(item.map(|item| ItemCapture { item, token }))
    }

    /// Read an article snapshot consistently, including an already adopted archive.
    /// Callers preparing a share must remove private fields before signing it.
    pub fn saved_article_snapshot(&self, id: &str) -> Result<Option<SavedItem>> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("article snapshot lock");
        let before = self.item_dirty_token(id)?;
        let item = self.saved_item_snapshot_unlocked(id)?;
        if before != self.item_dirty_token(id)? {
            return Err(invalid("article changed while preparing its snapshot"));
        }
        Ok(item)
    }

    pub(crate) fn saved_item_snapshot_unlocked(&self, id: &str) -> Result<Option<SavedItem>> {
        if self.private_item_removed(id)? {
            return Ok(None);
        }
        let params = id_params(id);
        let base = self.query_with_params("?[kind, title, creators, primary_url, published_at, language, summary, discovered_at] := *item{id: $id, kind, title, creators, primary_url, published_at, language, summary, discovered_at}", params.clone())?;
        let Some(row) = base.rows.first() else {
            return Ok(None);
        };
        if string(&row[0])? != "article" {
            return Err(invalid("unsupported saved-item kind"));
        }
        let DataValue::List(creators) = &row[2] else {
            return Err(invalid("invalid creator list"));
        };
        let article = self.query_with_params("?[feed_url, word_count, estimated_read_minutes, site_name, byline] := *item_article{item_id: $id, feed_url, word_count, estimated_read_minutes, site_name, byline}", params.clone())?;
        let identifiers = self.query_with_params("?[scheme, value, is_canonical, discovered_at] := *item_identifier{item_id: $id, scheme, value, is_canonical, discovered_at} :sort scheme, value", params.clone())?;
        let contents = self.query_with_params("?[format, blob_id, fetched_at, extracted_with, byte_size] := *item_content{item_id: $id, format, blob_id, fetched_at, extracted_with, byte_size} :sort format", params)?;
        let mut item = SavedItem {
            id: id.into(),
            kind: string(&row[0])?,
            title: string(&row[1])?,
            creators: creators.iter().map(string).collect::<Result<_>>()?,
            primary_url: nullable_string(&row[3])?,
            published_at_micros: nullable_time(&row[4])?,
            language: nullable_string(&row[5])?,
            summary: nullable_string(&row[6])?,
            discovered_at_micros: time(&row[7])?,
            article: article
                .rows
                .first()
                .map(|row| -> Result<_> {
                    Ok(ArticleMetadata {
                        feed_url: nullable_string(&row[0])?,
                        word_count: nullable_int(&row[1])?,
                        estimated_read_minutes: nullable_int(&row[2])?,
                        site_name: nullable_string(&row[3])?,
                        byline: nullable_string(&row[4])?,
                    })
                })
                .transpose()?,
            identifiers: identifiers
                .rows
                .iter()
                .map(|row| {
                    let DataValue::Bool(is_canonical) = row[2] else {
                        return Err(invalid("invalid canonical flag"));
                    };
                    Ok(ItemIdentifier {
                        scheme: string(&row[0])?,
                        value: string(&row[1])?,
                        is_canonical,
                        discovered_at_micros: time(&row[3])?,
                    })
                })
                .collect::<Result<_>>()?,
            contents: contents
                .rows
                .iter()
                .map(|row| {
                    Ok(ContentReference {
                        format: string(&row[0])?,
                        blob: BlobReference {
                            blob_id: string(&row[1])?,
                            byte_size: u64::try_from(int(&row[4])?)
                                .map_err(|_| invalid("negative cached size"))?,
                        },
                        fetched_at_micros: time(&row[2])?,
                        extracted_with: nullable_string(&row[3])?,
                    })
                })
                .collect::<Result<_>>()?,
            resources: Vec::new(),
            current_images: None,
        };
        if let Some(body) = item
            .contents
            .iter()
            .find(|content| content.format == "html_readable")
        {
            item.current_images = self.current_image_scope(id, &body.blob.blob_id)?;
            item.resources = item.current_images.clone().unwrap_or_default();
        }
        Ok(Some(item))
    }

    /// Sign a captured source only if its token still identifies the current data.
    pub fn adopt_private_item(
        &self,
        mut capture: ItemCapture,
        resources: Vec<BlobReference>,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<ItemAdoption> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private library write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        if self.item_dirty_token(&capture.item.id)?.as_ref() != Some(&capture.token)
            || self.private_item_removed(&capture.item.id)?
        {
            return Ok(ItemAdoption::Stale);
        }
        capture.item.resources = resources;
        capture.item.normalize();
        capture.item.validate()?;
        let heads = self.private_item_heads(&capture.item.id)?;
        if resolve_item_heads(&heads)?.as_ref() == Some(&capture.item) {
            self.query_with_params(
                "?[id] <- [[$id]] :rm self_item_dirty {id}",
                id_params(&capture.item.id),
            )?;
            return Ok(ItemAdoption::Unchanged);
        }
        let record = ItemRevision::sign(
            capture.item,
            heads.iter().map(|head| head.revision_id.clone()).collect(),
            at_micros,
            signer,
        )?;
        record.validate_parents(&heads)?;
        self.apply_private_item(&record, &heads, Some(&capture.token))?;
        Ok(ItemAdoption::Recorded(Box::new(record)))
    }

    /// Read verified causal heads, independent of wall-clock timestamps.
    pub fn private_item_heads(&self, id: &str) -> Result<Vec<ItemRevision>> {
        let rows = self.query_with_params(&format!("{HEAD_RULES}\n?[rev, encoded] := head[$id, rev], *self_item_revision{{id: $id, revision_id: rev, encoded}} :sort rev"), id_params(id))?;
        rows.rows
            .iter()
            .map(|row| decode_item(bytes(&row[1])?))
            .collect()
    }

    /// Verify original authorship and authenticate the live forwarding device.
    pub fn accept_self_item_revision(
        &self,
        record: &ItemRevision,
        sender: &PublicKey,
    ) -> Result<ItemReceive> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private library write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        record.verify_signature()?;
        if record.item.kind != "article" {
            return Err(invalid("unsupported saved-item kind"));
        }
        self.require_private_sender(sender, Some(&record.author))?;
        self.accept_item_unlocked(record)
    }

    fn accept_item_unlocked(&self, record: &ItemRevision) -> Result<ItemReceive> {
        let mut params = item_record_params(record)?;
        if self.private_item_removed(&record.item.id)? {
            self.discard_library_pending(&PrivateFact::Item(record.clone()).key()?)?;
            return Ok(ItemReceive::Removed);
        }
        let known = self.query_with_params(
            "?[encoded] := *self_item_revision{id: $id, revision_id: $rev, encoded}",
            params.clone(),
        )?;
        if let Some(row) = known.rows.first() {
            if decode_item(bytes(&row[0])?)? != *record {
                return Err(invalid("item revision collision"));
            }
            return Ok(ItemReceive::Existing);
        }
        params.insert(
            "wanted".into(),
            DataValue::List(
                record
                    .parents
                    .iter()
                    .map(|id| DataValue::List(vec![DataValue::from(id.as_str())]))
                    .collect(),
            ),
        );
        let foreign = self.query_with_params("wanted[rev] <- $wanted\n?[rev] := wanted[rev], *self_item_revision{id: owner, revision_id: rev}, owner != $id", params.clone())?;
        if !foreign.rows.is_empty() {
            return Err(invalid("item parent belongs to another article"));
        }
        let rows = self.query_with_params("wanted[rev] <- $wanted\n?[rev, encoded] := wanted[rev], *self_item_revision{id: $id, revision_id: rev, encoded} :sort rev", params)?;
        let parents = rows
            .rows
            .iter()
            .map(|row| decode_item(bytes(&row[1])?))
            .collect::<Result<Vec<_>>>()?;
        if parents.len() != record.parents.len()
            || self.item_dirty_token(&record.item.id)?.is_some()
        {
            self.stage_library_fact(&PrivateFact::Item(record.clone()), Some(&record.item.id))?;
            return Ok(ItemReceive::Pending);
        }
        record.validate_parents(&parents)?;
        self.apply_private_item(record, &self.private_item_heads(&record.item.id)?, None)?;
        Ok(ItemReceive::Applied)
    }

    /// Promote ready item revisions first, so a blocked prefix cannot starve them.
    pub fn promote_pending_items(&self) -> Result<Vec<String>> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private library write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        let rows = self.query(r#"
            blocked[key] := *self_library_incoming{key, kind: 'item', encoded, target}, *self_item_dirty{id: target}
            ?[key, encoded] := *self_library_incoming{key, kind: 'item', encoded}, not blocked[key]
            :sort key
        "#)?;
        let mut applied = Vec::new();
        for row in rows.rows {
            let key = string(&row[0])?;
            let record = decode_item(bytes(&row[1])?)?;
            // Staged facts were authenticated when received. Recheck the known
            // Root author without treating historical revocation as deletion.
            let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
            self.require_private_sender(&binding.device_pubkey, Some(&record.author))?;
            match self.accept_item_unlocked(&record) {
                Ok(ItemReceive::Applied) => applied.push(record.item.id),
                Ok(ItemReceive::Existing | ItemReceive::Removed) => {
                    self.discard_library_pending(&key)?
                }
                Ok(ItemReceive::Pending) => {}
                Err(StoreError::LibraryFact(_) | StoreError::InvalidInput { .. }) => {
                    self.discard_library_pending(&key)?
                }
                Err(error) => return Err(error),
            }
            if applied.len() >= 100 {
                break;
            }
        }
        Ok(applied)
    }

    /// Page exact immutable portable bytes awaiting Iroh publication.
    pub fn private_fact_outbox(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, PrivateFact)>> {
        let rows = self.query_with_params("?[key, encoded] := *self_fact_outbox{key, encoded}, ($first || key > $after) :sort key :limit $limit", BTreeMap::from([
            ("after".into(), DataValue::from(after.unwrap_or(""))), ("first".into(), DataValue::Bool(after.is_none())), ("limit".into(), DataValue::from(limit.min(1000) as i64)),
        ]))?;
        rows.rows
            .iter()
            .map(|row| {
                let key = string(&row[0])?;
                let fact = PrivateFact::from_bytes(bytes(&row[1])?)?;
                if fact.key()? != key {
                    return Err(invalid("mirror queue key mismatch"));
                }
                Ok((key, fact))
            })
            .collect()
    }

    /// Remove only the published content identity; a newer revision has another key.
    pub fn acknowledge_private_fact(&self, key: &str) -> Result<()> {
        self.query_with_params(
            "?[key] <- [[$key]] :rm self_fact_outbox {key}",
            BTreeMap::from([("key".into(), DataValue::from(key))]),
        )?;
        Ok(())
    }

    /// Rebuild a lost Iroh mirror from durable original portable records.
    ///
    /// Called for a new/recovered namespace, never on every background poll.
    pub fn rebuild_private_fact_outbox(&self) -> Result<()> {
        self.query("?[key, encoded] := *self_fact_history{key, encoded} :put self_fact_outbox {key => encoded}")?;
        Ok(())
    }

    /// Page declared cached content waiting for an integrity-checked local copy.
    pub fn private_blob_incoming(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BlobReference>> {
        let rows = self.query_with_params("?[blob_id, byte_size] := *self_blob_incoming{blob_id, byte_size}, ($first || blob_id > $after) :sort blob_id :limit $limit", BTreeMap::from([
            ("after".into(), DataValue::from(after.unwrap_or(""))), ("first".into(), DataValue::Bool(after.is_none())), ("limit".into(), DataValue::from(limit.min(1000) as i64)),
        ]))?;
        rows.rows
            .iter()
            .map(|row| {
                Ok(BlobReference {
                    blob_id: string(&row[0])?,
                    byte_size: u64::try_from(int(&row[1])?)
                        .map_err(|_| invalid("negative incoming blob size"))?,
                })
            })
            .collect()
    }

    /// Acknowledge an exact declared hash/length after atomic cache installation.
    pub fn acknowledge_private_blob(&self, reference: &BlobReference) -> Result<()> {
        self.query_with_params("?[blob_id] := *self_blob_incoming{blob_id, byte_size}, blob_id = $id, byte_size = $size :rm self_blob_incoming {blob_id}", BTreeMap::from([
            ("id".into(), DataValue::from(reference.blob_id.as_str())), ("size".into(), DataValue::from(reference.byte_size as i64)),
        ]))?;
        Ok(())
    }

    /// Requeue declared content after a cache loss without changing signed history.
    pub fn restore_private_blob_queue(&self) -> Result<()> {
        let mut after = None;
        loop {
            let rows = self.query_with_params("?[key, encoded] := *self_fact_history{key, encoded}, starts_with(key, 'item/'), ($first || key > $after) :sort key :limit 100", BTreeMap::from([
                ("after".into(), DataValue::from(after.as_deref().unwrap_or(""))), ("first".into(), DataValue::Bool(after.is_none())),
            ]))?;
            if rows.rows.is_empty() {
                break;
            }
            for row in rows.rows {
                let fact = PrivateFact::from_bytes(bytes(&row[1])?)?;
                self.query_with_params(QUEUE_BLOBS, private_fact_params(&fact)?)?;
                after = Some(string(&row[0])?);
            }
        }
        Ok(())
    }

    pub(crate) fn require_private_sender(
        &self,
        sender: &PublicKey,
        author: Option<&PublicKey>,
    ) -> Result<()> {
        let devices = self.self_devices()?;
        if !devices
            .iter()
            .any(|row| row.certificate.device_pubkey == *sender && row.is_authorized())
            || author.is_some_and(|author| {
                !devices
                    .iter()
                    .any(|row| row.certificate.device_pubkey == *author)
            })
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        Ok(())
    }

    pub(crate) fn stage_library_fact(
        &self,
        fact: &PrivateFact,
        target: Option<&str>,
    ) -> Result<()> {
        let mut params = private_fact_params(fact)?;
        params.insert("pf_kind".into(), DataValue::from(fact.kind()));
        params.insert("pf_target".into(), optional_string(target));
        self.query_with_params(&format!("{{ ?[key, encoded, kind, target] <- [[$pf_key, $pf_encoded, $pf_kind, $pf_target]] :put self_library_incoming {{key => encoded, kind, target}} }}\n{QUEUE_BLOBS}"), params)?;
        Ok(())
    }

    pub(crate) fn discard_library_pending(&self, key: &str) -> Result<()> {
        self.query_with_params(
            "?[key] <- [[$key]] :rm self_library_incoming {key}",
            BTreeMap::from([("key".into(), DataValue::from(key))]),
        )?;
        Ok(())
    }

    fn item_dirty_token(&self, id: &str) -> Result<Option<String>> {
        self.query_with_params(
            "?[token] := *self_item_dirty{id: $id, token}",
            id_params(id),
        )?
        .rows
        .first()
        .map(|row| string(&row[0]))
        .transpose()
    }

    fn private_item_removed(&self, id: &str) -> Result<bool> {
        Ok(!self
            .query_with_params(
                "?[id] := *entity_tombstone{id @ \"END\"}, id = $id",
                id_params(id),
            )?
            .rows
            .is_empty())
    }

    fn apply_private_item(
        &self,
        record: &ItemRevision,
        current: &[ItemRevision],
        expected_token: Option<&str>,
    ) -> Result<()> {
        let mut heads: Vec<_> = current
            .iter()
            .filter(|head| !record.parents.contains(&head.revision_id))
            .cloned()
            .collect();
        heads.push(record.clone());
        let item = resolve_item_heads(&heads)?.expect("one or more item heads");
        let mut params = item_record_params(record)?;
        params.insert("expected_token".into(), optional_string(expected_token));
        params.insert(
            "base".into(),
            DataValue::List(vec![DataValue::List(vec![
                DataValue::from(item.id.as_str()),
                DataValue::from(item.kind.as_str()),
                DataValue::from(item.title.as_str()),
                string_list(&item.creators),
                optional_string(item.primary_url.as_deref()),
                optional_validity(item.published_at_micros),
                optional_string(item.language.as_deref()),
                optional_string(item.summary.as_deref()),
                validity(item.discovered_at_micros),
            ])]),
        );
        let article = item
            .article
            .as_ref()
            .map(|article| {
                DataValue::List(vec![
                    DataValue::from(item.id.as_str()),
                    optional_string(article.feed_url.as_deref()),
                    optional_int(article.word_count),
                    optional_int(article.estimated_read_minutes),
                    optional_string(article.site_name.as_deref()),
                    optional_string(article.byline.as_deref()),
                ])
            })
            .into_iter()
            .collect();
        params.insert("article".into(), DataValue::List(article));
        params.insert(
            "identifiers".into(),
            DataValue::List(
                item.identifiers
                    .iter()
                    .map(|id| {
                        DataValue::List(vec![
                            DataValue::from(item.id.as_str()),
                            DataValue::from(id.scheme.as_str()),
                            DataValue::from(id.value.as_str()),
                            DataValue::Bool(id.is_canonical),
                            validity(id.discovered_at_micros),
                        ])
                    })
                    .collect(),
            ),
        );
        params.insert(
            "contents".into(),
            DataValue::List(
                item.contents
                    .iter()
                    .map(|content| {
                        DataValue::List(vec![
                            DataValue::from(item.id.as_str()),
                            DataValue::from(content.format.as_str()),
                            DataValue::from(content.blob.blob_id.as_str()),
                            validity(content.fetched_at_micros),
                            optional_string(content.extracted_with.as_deref()),
                            DataValue::from(content.blob.byte_size as i64),
                        ])
                    })
                    .collect(),
            ),
        );
        let image_scope = item
            .contents
            .iter()
            .find(|body| body.format == "html_readable")
            .zip(item.current_images.as_ref())
            .map(|(body, images)| -> Result<DataValue> {
                Ok(DataValue::List(vec![
                    DataValue::from(item.id.as_str()),
                    DataValue::from(body.blob.blob_id.as_str()),
                    DataValue::Json(JsonData(
                        serde_json::to_value(images)
                            .map_err(|_| invalid("invalid current image scope"))?,
                    )),
                ]))
            })
            .transpose()?
            .into_iter()
            .collect();
        params.insert("image_scope".into(), DataValue::List(image_scope));
        self.query_with_params(&format!(r#"
            {{ ?[id] := *self_item_dirty{{id: $id, token}}, id = $id, (is_null($expected_token) || token != $expected_token) :assert none }}
            {{ ?[id, revision_id, encoded, parents] <- [[$id, $rev, $encoded, $parents]] :put self_item_revision {{id, revision_id => encoded, parents}} }}
            {{ ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- $base
               :put item {{id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}} }}
            {{ ?[item_id] := *item_article{{item_id: $id}}, item_id = $id :rm item_article {{item_id}} }}
            {{ ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] <- $article
               :put item_article {{item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}} }}
            {{ ?[item_id, scheme, value] := *item_identifier{{item_id: $id, scheme, value}}, item_id = $id :rm item_identifier {{item_id, scheme, value}} }}
            {{ ?[item_id, scheme, value, is_canonical, discovered_at] <- $identifiers :put item_identifier {{item_id, scheme, value => is_canonical, discovered_at}} }}
            {{ ?[item_id, format] := *item_content{{item_id: $id, format}}, item_id = $id :rm item_content {{item_id, format}} }}
            {{ ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <- $contents :put item_content {{item_id, format => blob_id, fetched_at, extracted_with, byte_size}} }}
            {{ ?[item_id, blob_id, images] <- $image_scope :put item_current_image_scope {{item_id, blob_id => images}} }}
            {QUEUE_PRIVATE_FACT}
            {QUEUE_BLOBS}
            {{ ?[key] <- [[$pf_key]] :rm self_library_incoming {{key}} }}
            {{ ?[id] <- [[$id]] :rm self_item_dirty {{id}} }}
        "#), params)?;
        Ok(())
    }
}

pub(crate) fn private_fact_params(fact: &PrivateFact) -> Result<BTreeMap<String, DataValue>> {
    let blobs = match fact {
        PrivateFact::Item(record) => record.item.blobs(),
        _ => Vec::new(),
    };
    Ok(BTreeMap::from([
        ("pf_key".into(), DataValue::from(fact.key()?)),
        ("pf_encoded".into(), DataValue::Bytes(fact.to_bytes()?)),
        (
            "pf_blobs".into(),
            DataValue::List(
                blobs
                    .into_iter()
                    .map(|blob| {
                        DataValue::List(vec![
                            DataValue::from(blob.blob_id),
                            DataValue::from(blob.byte_size as i64),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}
fn item_record_params(record: &ItemRevision) -> Result<BTreeMap<String, DataValue>> {
    let fact = PrivateFact::Item(record.clone());
    let mut params = private_fact_params(&fact)?;
    params.extend([
        ("id".into(), DataValue::from(record.item.id.as_str())),
        ("rev".into(), DataValue::from(record.revision_id.as_str())),
        ("encoded".into(), DataValue::Bytes(fact.to_bytes()?)),
        ("parents".into(), string_list(&record.parents)),
    ]);
    Ok(params)
}
fn decode_item(encoded: &[u8]) -> Result<ItemRevision> {
    match PrivateFact::from_bytes(encoded)? {
        PrivateFact::Item(record) => Ok(record),
        _ => Err(invalid("invalid item history kind")),
    }
}
fn id_params(id: &str) -> BTreeMap<String, DataValue> {
    BTreeMap::from([("id".into(), DataValue::from(id))])
}
fn string_list(values: &[String]) -> DataValue {
    DataValue::List(
        values
            .iter()
            .map(|value| DataValue::from(value.as_str()))
            .collect(),
    )
}
fn optional_string(value: Option<&str>) -> DataValue {
    value.map_or(DataValue::Null, DataValue::from)
}
fn optional_int(value: Option<i64>) -> DataValue {
    value.map_or(DataValue::Null, DataValue::from)
}
fn validity(value: i64) -> DataValue {
    DataValue::Validity(Validity::from((value, true)))
}
fn optional_validity(value: Option<i64>) -> DataValue {
    value.map_or(DataValue::Null, validity)
}
fn string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        _ => Err(invalid("invalid stored string")),
    }
}
fn nullable_string(value: &DataValue) -> Result<Option<String>> {
    if *value == DataValue::Null {
        Ok(None)
    } else {
        string(value).map(Some)
    }
}
fn int(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Num(cozo::Num::Int(value)) => Ok(*value),
        _ => Err(invalid("invalid stored integer")),
    }
}
fn nullable_int(value: &DataValue) -> Result<Option<i64>> {
    if *value == DataValue::Null {
        Ok(None)
    } else {
        int(value).map(Some)
    }
}
fn time(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Validity(value) => Ok(value.timestamp.0.0),
        _ => Err(invalid("invalid stored timestamp")),
    }
}
fn nullable_time(value: &DataValue) -> Result<Option<i64>> {
    if *value == DataValue::Null {
        Ok(None)
    } else {
        time(value).map(Some)
    }
}
fn public_key(value: &DataValue) -> Result<PublicKey> {
    let raw: [u8; 32] = bytes(value)?
        .try_into()
        .map_err(|_| invalid("invalid stored public key"))?;
    Ok(PublicKey::from_bytes(raw))
}
fn signature(value: &DataValue) -> Result<Signature> {
    let raw: [u8; 64] = bytes(value)?
        .try_into()
        .map_err(|_| invalid("invalid stored signature"))?;
    Ok(Signature::from_bytes(raw))
}
fn invalid(message: &'static str) -> StoreError {
    StoreError::InvalidInput {
        context: "private library",
        message: message.into(),
    }
}
