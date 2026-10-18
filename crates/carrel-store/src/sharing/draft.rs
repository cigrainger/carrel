//! Durable installation-local drafts. No operation here implicitly publishes text.
use super::*;
use carrel_core::conversation::{
    ConversationContent as Content, ConversationRecord, ReplyDraft, ReplyDraftInput,
    ReplyDraftSnapshot, ThreadScope,
};

/// Confirmation of an exact local Send, with the current durable composer state.
#[derive(Clone, Debug)]
pub struct ReplyDraftSent {
    /// Original signed proof, also returned after a lost acknowledgement.
    pub record: ConversationRecord,
    /// Current state; acknowledging an earlier Send never clears a subsequent draft.
    pub draft: ReplyDraftSnapshot,
    /// The exact proof had already committed; no delivery work was recreated.
    pub already_sent: bool,
}

impl Store {
    /// Read a retained draft without requiring an active operational certificate.
    pub fn load_reply_draft(&self, share_id: &str) -> Result<ReplyDraftSnapshot> {
        let _identity = self.identity_lock.lock().expect("draft read identity lock");
        let _social = self.social_lock.lock().expect("draft read lock");
        let (root, _, local_device) = self.local_draft_root_unlocked(share_id)?;
        let snapshot = self.reply_draft_unlocked(share_id)?;
        if let Some(draft) = &snapshot.draft {
            if draft.retry_device != local_device {
                return Err(invalid(
                    "draft retry key differs from the local installation",
                ));
            }
            self.require_draft_parent_unlocked(
                &ThreadScope::from_share(&root)?,
                draft.parent.as_deref(),
            )?;
        }
        Ok(snapshot)
    }

    /// Compare and save exact local composition; the native caller supplies its key.
    /// A replay of the immediately preceding exact save acknowledges its stored result.
    pub fn save_reply_draft(
        &self,
        share_id: &str,
        expected_revision: i64,
        input: ReplyDraftInput,
        retry_device: &PublicKey,
    ) -> Result<ReplyDraftSnapshot> {
        input.verify()?;
        require_revision(expected_revision)?;
        let _identity = self.identity_lock.lock().expect("draft save identity lock");
        let _social = self.social_lock.lock().expect("draft save lock");
        let (root, master, local_device) = self.local_draft_root_unlocked(share_id)?;
        if local_device != *retry_device {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let draft = ReplyDraft {
            body: input.body,
            parent: input.parent,
            request_id: input.request_id,
            retry_device: *retry_device,
        };
        draft.verify()?;
        let current = self.reply_draft_unlocked(share_id)?;
        if current.draft.as_ref() == Some(&draft)
            && (current.revision == expected_revision
                || expected_revision.checked_add(1) == Some(current.revision))
        {
            return Ok(current);
        }
        if current.revision != expected_revision {
            return Err(conflict(share_id, expected_revision, current.revision));
        }
        self.require_draft_parent_unlocked(
            &ThreadScope::from_share(&root)?,
            draft.parent.as_deref(),
        )?;
        // A completed request must not become a new composition after its response
        // was lost. Recovery has a fresh uncertified key and no old request identity.
        if let Some(cert) = self.device_certificate(&master, retry_device)? {
            let id = Share::draft_id(&cert, &draft.request_id)?;
            if self.social_record(&format!("reply/{id}"))?.is_some() {
                return Err(invalid(
                    "reply retry token was already sent; retain the text and start a new retry identity",
                ));
            }
        }
        let next = ReplyDraftSnapshot {
            revision: next_revision(share_id, current.revision)?,
            draft: Some(draft),
        };
        let (script, params) = draft_transaction(share_id, current.revision, &next);
        self.query_with_params(&script, params)?;
        Ok(next)
    }

    /// Explicitly discard only the reviewed revision, retaining its clear tombstone.
    pub fn discard_reply_draft(
        &self,
        share_id: &str,
        expected_revision: i64,
    ) -> Result<ReplyDraftSnapshot> {
        require_revision(expected_revision)?;
        let _identity = self
            .identity_lock
            .lock()
            .expect("draft discard identity lock");
        let _social = self.social_lock.lock().expect("draft discard lock");
        self.local_draft_root_unlocked(share_id)?;
        let current = self.reply_draft_unlocked(share_id)?;
        if current.revision != expected_revision {
            // A lost discard response may acknowledge only the immediately
            // following tombstone, never erase a subsequently saved composition.
            if current.draft.is_none() && expected_revision.checked_add(1) == Some(current.revision)
            {
                return Ok(current);
            }
            return Err(conflict(share_id, expected_revision, current.revision));
        }
        // Even an empty slot needs a new fence: composition may exist only in a
        // delayed autosave carrying this revision, including revision zero.
        let next = ReplyDraftSnapshot {
            revision: next_revision(share_id, current.revision)?,
            draft: None,
        };
        let (script, params) = draft_transaction(share_id, current.revision, &next);
        self.query_with_params(&script, params)?;
        Ok(next)
    }

    /// Queue a reply and clear its exact persisted draft in one transaction.
    /// Existing exact proofs never requeue; an exact legacy draft may be cleared.
    pub fn reply_from_draft(
        &self,
        share_id: &str,
        submitted: &ReplyDraftSnapshot,
        at: i64,
        device: &Keypair,
    ) -> Result<ReplyDraftSent> {
        submitted.verify()?;
        let draft = submitted
            .draft
            .as_ref()
            .ok_or_else(|| invalid("no reply draft was submitted"))?;
        let _private = self
            .private_state_lock
            .lock()
            .expect("draft send consent lock");
        let _identity = self.identity_lock.lock().expect("draft send identity lock");
        let _social = self.social_lock.lock().expect("draft send lock");
        let (root, _, local_device) = self.local_draft_root_unlocked(share_id)?;
        let cert = self.social_local_certificate(device.public())?;
        if draft.retry_device != *device.public() || draft.retry_device != local_device {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let scope = ThreadScope::from_share(&root)?;
        let id = Share::draft_id(&cert, &draft.request_id)?;
        let current = self.reply_draft_unlocked(share_id)?;
        if let Some(ShareFact::Conversation(record)) = self.social_record(&format!("reply/{id}"))? {
            if matches!(&record.content, Content::Reply { scope: known, id: known_id, request_id, parent, body }
                if *known == scope && *known_id == id && *request_id == draft.request_id
                    && *parent == draft.parent && *body == draft.body)
                && record.certificate.master_pubkey == cert.master_pubkey
                && record.certificate.device_pubkey == cert.device_pubkey
            {
                // The compatibility API may have committed this exact reply
                // without clearing a previously persisted draft. Acknowledge
                // that row locally, but never clear a later composition.
                let acknowledged = if current == *submitted {
                    let next = ReplyDraftSnapshot {
                        revision: next_revision(share_id, current.revision)?,
                        draft: None,
                    };
                    let (script, params) = draft_transaction(share_id, current.revision, &next);
                    self.query_with_params(&script, params)?;
                    next
                } else {
                    current
                };
                return Ok(ReplyDraftSent {
                    record: *record,
                    draft: acknowledged,
                    already_sent: true,
                });
            }
            return Err(invalid(
                "reply retry changed its original content; retain the text and use a new retry identity",
            ));
        }
        if current != *submitted {
            return Err(conflict(share_id, submitted.revision, current.revision));
        }
        if self.social_share_retracted(share_id, &scope.sharer, &root.recipient)? {
            return Err(invalid("this share has been withdrawn"));
        }
        let participation = self.conversation_participation_unlocked(&scope)?;
        if participation != ConversationParticipation::Admitted {
            return Err(StoreError::ConversationUnavailable(participation));
        }
        self.require_draft_parent_unlocked(&scope, draft.parent.as_deref())?;
        let record = ConversationRecord::sign(
            Content::Reply {
                scope,
                id,
                request_id: draft.request_id.clone(),
                parent: draft.parent.clone(),
                body: draft.body.clone(),
            },
            at,
            cert,
            device,
        )?;
        let next = ReplyDraftSnapshot {
            revision: next_revision(share_id, current.revision)?,
            draft: None,
        };
        let (mut script, mut params) =
            super::conversation::accepted_conversation_transaction(&record, true)?;
        let (clear, values) = draft_transaction(share_id, current.revision, &next);
        script.push_str(&clear);
        params.extend(values);
        self.query_with_params(&script, params)?;
        self.social_changed();
        Ok(ReplyDraftSent {
            record,
            draft: next,
            already_sent: false,
        })
    }

    fn local_draft_root_unlocked(&self, share_id: &str) -> Result<(Share, PublicKey, PublicKey)> {
        let root = self.conversation_root(share_id)?;
        let scope = ThreadScope::from_share(&root)?;
        let (master, device) = if let Some(binding) = self.self_identity()? {
            (binding.master_pubkey, binding.device_pubkey)
        } else {
            let origin = self.restore_origin()?.ok_or(StoreError::IdentityUnbound)?;
            (
                origin.master_pubkey.ok_or(StoreError::IdentityUnbound)?,
                origin.device_pubkey,
            )
        };
        if !scope.contains(&master) {
            return Err(invalid(
                "draft conversation is outside the local reading identity",
            ));
        }
        Ok((root, master, device))
    }

    fn require_draft_parent_unlocked(
        &self,
        scope: &ThreadScope,
        parent: Option<&str>,
    ) -> Result<()> {
        if let Some(id) = parent {
            let Some(ShareFact::Conversation(record)) =
                self.social_record(&format!("reply/{id}"))?
            else {
                return Err(invalid(
                    "draft parent is unavailable; retain the text and review its reply context",
                ));
            };
            if record.scope() != Some(scope)
                || !matches!(&record.content, Content::Reply { id: known, .. } if known == id)
            {
                return Err(invalid("draft parent belongs to a different conversation"));
            }
        }
        Ok(())
    }

    fn reply_draft_unlocked(&self, share_id: &str) -> Result<ReplyDraftSnapshot> {
        let rows = self.query_with_params(
            "?[revision,body,parent,request_id,retry_device] := *social_reply_draft{share_id:$draft_share,revision,body,parent,request_id,retry_device}",
            BTreeMap::from([("draft_share".into(), DataValue::from(share_id))]),
        )?;
        let Some(row) = rows.rows.first() else {
            return Ok(ReplyDraftSnapshot::default());
        };
        let revision = integer(&row[0])?;
        if revision <= 0 {
            return Err(invalid("invalid stored draft revision"));
        }
        let draft = if row[1] == DataValue::Null {
            if row[2..].iter().any(|value| *value != DataValue::Null) {
                return Err(invalid("invalid cleared draft payload"));
            }
            None
        } else {
            Some(ReplyDraft {
                body: string(&row[1])?,
                parent: if row[2] == DataValue::Null {
                    None
                } else {
                    Some(string(&row[2])?)
                },
                request_id: string(&row[3])?,
                retry_device: public_key(&row[4])?,
            })
        };
        let snapshot = ReplyDraftSnapshot { revision, draft };
        snapshot.verify()?;
        Ok(snapshot)
    }

    /// Validate unsigned local state as well as the signed backup records.
    pub(crate) fn validate_local_reply_drafts(&self) -> Result<()> {
        for row in self
            .query("?[share_id] := *social_reply_draft{share_id} :sort share_id")?
            .rows
        {
            let share_id = string(&row[0])?;
            let (root, _, device) = self.local_draft_root_unlocked(&share_id)?;
            if let Some(draft) = self.reply_draft_unlocked(&share_id)?.draft {
                if draft.retry_device != device {
                    return Err(invalid(
                        "draft retry key differs from the local installation",
                    ));
                }
                self.require_draft_parent_unlocked(
                    &ThreadScope::from_share(&root)?,
                    draft.parent.as_deref(),
                )?;
            }
        }
        Ok(())
    }

    /// Called only after isolated restore preparation installs its fresh local key.
    pub(crate) fn rekey_restored_reply_drafts(&self, fresh: &PublicKey) -> Result<()> {
        let origin = self.restore_origin()?.ok_or(StoreError::IdentityUnbound)?;
        if self.self_identity()?.is_some() || origin.device_pubkey != *fresh {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let mut rows = Vec::new();
        for row in self
            .query("?[share_id] := *social_reply_draft{share_id} :sort share_id")?
            .rows
        {
            let share_id = string(&row[0])?;
            self.local_draft_root_unlocked(&share_id)?;
            let mut snapshot = self.reply_draft_unlocked(&share_id)?;
            if let Some(draft) = snapshot.draft.as_mut() {
                let bytes = carrel_core::identity::canonical_bytes(&(
                    "carrel-restored-reply-draft-v1",
                    &share_id,
                    snapshot.revision,
                    &draft.request_id,
                    fresh,
                ))
                .map_err(|_| invalid("could not encode recovered draft identity"))?;
                draft.request_id = blake3::hash(&bytes).to_hex().to_string();
                draft.retry_device = *fresh;
            }
            snapshot.revision = next_revision(&share_id, snapshot.revision)?;
            rows.push(DataValue::List(draft_row(&share_id, &snapshot)));
        }
        if !rows.is_empty() {
            self.query_with_params(
                "?[share_id,revision,body,parent,request_id,retry_device] <- $draft_rows :put social_reply_draft {share_id=>revision,body,parent,request_id,retry_device}",
                BTreeMap::from([("draft_rows".into(), DataValue::List(rows))]),
            )?;
        }
        Ok(())
    }
}

fn require_revision(revision: i64) -> Result<()> {
    if revision < 0 {
        return Err(invalid("draft revision must be nonnegative"));
    }
    Ok(())
}

fn next_revision(share_id: &str, revision: i64) -> Result<i64> {
    revision
        .checked_add(1)
        .ok_or_else(|| StoreError::ReplyDraftRevisionExhausted {
            share_id: share_id.into(),
        })
}

fn conflict(share_id: &str, expected_revision: i64, current_revision: i64) -> StoreError {
    StoreError::ReplyDraftConflict {
        share_id: share_id.into(),
        expected_revision,
        current_revision,
    }
}

fn draft_row(share_id: &str, snapshot: &ReplyDraftSnapshot) -> Vec<DataValue> {
    let mut row = vec![
        DataValue::from(share_id),
        DataValue::from(snapshot.revision),
    ];
    if let Some(draft) = &snapshot.draft {
        row.extend([
            DataValue::from(draft.body.as_str()),
            optional(draft.parent.as_deref()),
            DataValue::from(draft.request_id.as_str()),
            key_value(draft.retry_device),
        ]);
    } else {
        row.extend([
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
        ]);
    }
    row
}

fn draft_transaction(
    share_id: &str,
    expected: i64,
    next: &ReplyDraftSnapshot,
) -> (String, BTreeMap<String, DataValue>) {
    let script = r#"{
        current[rev] := *social_reply_draft{share_id:$draft_share,revision:rev}
        current[rev] := not *social_reply_draft{share_id:$draft_share}, rev=0
        ?[rev] := current[rev], rev != $draft_expected :assert none
    }
    { ?[share_id,revision,body,parent,request_id,retry_device] <- $draft_rows
      :put social_reply_draft {share_id=>revision,body,parent,request_id,retry_device} }
    "#
    .to_owned();
    (
        script,
        BTreeMap::from([
            ("draft_share".into(), DataValue::from(share_id)),
            ("draft_expected".into(), DataValue::from(expected)),
            (
                "draft_rows".into(),
                DataValue::List(vec![DataValue::List(draft_row(share_id, next))]),
            ),
        ]),
    )
}
