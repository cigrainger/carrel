//! Consent is interpreted from signed history; local projections never grant it.
use super::*;
use carrel_core::conversation::{
    AdmissionBasis, ConversationControl, ConversationControls, ThreadScope,
    conversation_control_key, resolve_conversation_controls,
};
use carrel_core::private_state::{StateRevision, StateValue};

impl Store {
    /// Engine-confirmed preference. Unresolved ancestry cannot enable introductions.
    pub fn allow_unfollowed_group_conversations(&self) -> Result<bool> {
        let _private = self.private_state_lock.lock().expect("consent read lock");
        Ok(!self.pending_conversation_controls_unlocked()?
            && self
                .conversation_controls_unlocked()?
                .allow_unfollowed_groups)
    }

    /// Sign an explicit preference after observing every available control head.
    pub fn set_allow_unfollowed_group_conversations(
        &self,
        allow: bool,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let _private = self.private_state_lock.lock().expect("consent state lock");
        let _identity = self.identity_lock.lock().expect("consent identity lock");
        let _social = self.social_lock.lock().expect("consent social lock");
        self.social_local_certificate(device.public())?;
        self.require_resolved_conversation_controls_unlocked()?;
        self.adopt_legacy_social_conversations_unlocked(at, device)?;
        self.write_conversation_control_unlocked(
            ConversationControl::Policy {
                allow_unfollowed_groups: allow,
            },
            at,
            device,
        )
    }

    /// Private participation, independent of the root author's withdrawal.
    pub fn conversation_participation(&self, share_id: &str) -> Result<ConversationParticipation> {
        let _private = self
            .private_state_lock
            .lock()
            .expect("participation read lock");
        let _identity = self
            .identity_lock
            .lock()
            .expect("participation identity lock");
        let _social = self.social_lock.lock().expect("participation social lock");
        let scope = ThreadScope::from_share(&self.conversation_root(share_id)?)?;
        self.conversation_participation_unlocked(&scope)
    }

    /// Permanently leave this root without withdrawing it or changing its audience.
    pub fn leave_social_conversation(
        &self,
        share_id: &str,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let _private = self.private_state_lock.lock().expect("leave state lock");
        let _identity = self.identity_lock.lock().expect("leave identity lock");
        let _social = self.social_lock.lock().expect("leave social lock");
        self.social_local_certificate(device.public())?;
        self.adopt_legacy_social_conversations_unlocked(at, device)?;
        let root = self.conversation_root(share_id)?;
        let scope = ThreadScope::from_share(&root)?;
        let participation = self.conversation_participation_unlocked(&scope)?;
        if participation == ConversationParticipation::Left {
            return Ok(());
        }
        if participation != ConversationParticipation::Admitted {
            return Err(StoreError::ConversationUnavailable(participation));
        }
        self.write_conversation_control_unlocked(ConversationControl::Leave { scope }, at, device)
    }

    /// Recheck old mirror entries too: leaving stops reply bodies, not history controls.
    pub fn social_record_servable(&self, fact: &ShareFact) -> Result<bool> {
        fact.verify()?;
        let _private = self
            .private_state_lock
            .lock()
            .expect("serving consent lock");
        let _identity = self.identity_lock.lock().expect("serving identity lock");
        let _social = self.social_lock.lock().expect("serving social lock");
        let Some(stored) = self.social_record(&fact.key()?)? else {
            return Ok(false);
        };
        if stored != *fact {
            return Err(invalid("mirrored record differs from accepted proof"));
        }
        if let ShareFact::Conversation(record) = fact
            && let carrel_core::conversation::ConversationContent::Reply { scope, .. } =
                &record.content
        {
            if self.conversation_participation_unlocked(scope)?
                != ConversationParticipation::Admitted
            {
                return Ok(false);
            }
            let root = self.conversation_root(&scope.share_id)?;
            return Ok(!self.social_share_retracted(&root.id, &scope.sharer, &root.recipient)?);
        }
        Ok(true)
    }

    /// Adopt only the migration's exact accepted roots, never incoming replay.
    pub fn adopt_legacy_social_conversations(&self, at: i64, device: &Keypair) -> Result<usize> {
        let _private = self.private_state_lock.lock().expect("legacy consent lock");
        let _identity = self.identity_lock.lock().expect("legacy identity lock");
        let _social = self.social_lock.lock().expect("legacy social lock");
        self.social_local_certificate(device.public())?;
        self.adopt_legacy_social_conversations_unlocked(at, device)
    }

    pub(super) fn adopt_legacy_social_conversations_unlocked(
        &self,
        at: i64,
        device: &Keypair,
    ) -> Result<usize> {
        let rows = self.query("?[scope] := *social_conversation_legacy{scope} :sort scope")?;
        let mut adopted = 0;
        for row in rows.rows {
            let DataValue::Bytes(bytes) = &row[0] else {
                return Err(invalid("invalid legacy conversation scope"));
            };
            let scope = ThreadScope::from_bytes(bytes)?;
            let controls = self.conversation_controls_unlocked()?;
            if controls.conversations.contains_key(&scope.share_id) {
                continue;
            }
            self.write_conversation_control_unlocked(
                ConversationControl::Admit {
                    scope,
                    basis: AdmissionBasis::LegacyAccepted,
                },
                at,
                device,
            )?;
            adopted += 1;
        }
        Ok(adopted)
    }

    pub(crate) fn conversation_control_history_unlocked(&self) -> Result<Vec<StateRevision>> {
        self.query("?[encoded] := *self_state{kind:'conversation_control',encoded} :sort encoded")?
            .rows
            .iter()
            .map(|row| {
                let DataValue::Bytes(bytes) = &row[0] else {
                    return Err(invalid("invalid conversation control"));
                };
                Ok(StateRevision::from_bytes(bytes)?)
            })
            .collect()
    }

    pub(super) fn conversation_controls_unlocked(&self) -> Result<ConversationControls> {
        Ok(resolve_conversation_controls(
            &self.conversation_control_history_unlocked()?,
        )?)
    }

    pub(super) fn pending_conversation_controls_unlocked(&self) -> Result<bool> {
        Ok(!self.query("?[revision_id] := *self_state_incoming{kind:'conversation_control',revision_id} :limit 1")?.rows.is_empty())
    }

    fn require_resolved_conversation_controls_unlocked(&self) -> Result<()> {
        if self.pending_conversation_controls_unlocked()? {
            return Err(StoreError::ConversationUnavailable(
                ConversationParticipation::Pending,
            ));
        }
        Ok(())
    }

    pub(super) fn conversation_participation_unlocked(
        &self,
        scope: &ThreadScope,
    ) -> Result<ConversationParticipation> {
        let local = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if !scope.contains(&local.master_pubkey) {
            return Err(invalid("conversation is outside the bound identity"));
        }
        let controls = self.conversation_controls_unlocked()?;
        if let Some(decision) = controls.conversations.get(&scope.share_id) {
            if decision.scope != *scope {
                return Err(invalid("conversation consent names a different root"));
            }
            if matches!(
                decision.participation,
                ConversationParticipation::Left | ConversationParticipation::Declined
            ) {
                return Ok(decision.participation);
            }
            if !self.pending_conversation_controls_unlocked()? {
                return Ok(decision.participation);
            }
        }
        Ok(ConversationParticipation::Pending)
    }

    /// Validate self controls without waiting for the external social root.
    pub(crate) fn validate_conversation_control_unlocked(
        &self,
        record: &StateRevision,
    ) -> Result<()> {
        if let StateValue::ConversationControl(operation) = &record.value
            && let Some(scope) = operation.scope()
        {
            let local = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
            if !scope.contains(&local.master_pubkey) {
                return Err(invalid("control is outside the bound identity"));
            }
            if let Some(ShareFact::Share(root)) =
                self.social_record(&format!("share/{}", scope.share_id))?
                && ThreadScope::from_share(&root)? != *scope
            {
                return Err(invalid("control differs from original root"));
            }
        }
        Ok(())
    }

    pub(super) fn verify_replayed_root_scope_unlocked(&self, root: &Share) -> Result<()> {
        let scope = ThreadScope::from_share(root)?;
        if let Some(decision) = self
            .conversation_controls_unlocked()?
            .conversations
            .get(&scope.share_id)
            && decision.scope != scope
        {
            return Err(invalid("replayed root differs from private consent"));
        }
        Ok(())
    }

    fn write_conversation_control_unlocked(
        &self,
        operation: ConversationControl,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let (script, params) = self.prepare_conversation_control_unlocked(operation, at, device)?;
        self.query_with_params(&script, params)?;
        self.social_changed();
        Ok(())
    }

    fn prepare_conversation_control_unlocked(
        &self,
        operation: ConversationControl,
        at: i64,
        device: &Keypair,
    ) -> Result<(String, BTreeMap<String, DataValue>)> {
        let key = conversation_control_key();
        let heads = self.private_state_heads(&key)?;
        let record = StateRevision::sign(
            key,
            StateValue::ConversationControl(operation),
            heads.iter().map(|h| h.revision_id.clone()).collect(),
            at,
            device,
        )?;
        self.prepare_state(&record, &heads)
    }

    /// Extend the caller's root transaction, rather than committing admission first.
    pub(super) fn append_social_admission_unlocked(
        &self,
        script: &mut String,
        params: &mut BTreeMap<String, DataValue>,
        root: &Share,
        at: i64,
        device: &Keypair,
        suffix: &str,
    ) -> Result<()> {
        self.social_local_certificate(device.public())?;
        self.require_resolved_conversation_controls_unlocked()?;
        let controls = self.conversation_controls_unlocked()?;
        if let Some(operation) = self.social_admission_operation_unlocked(root, &controls)? {
            let (addition, values) =
                self.prepare_conversation_control_unlocked(operation, at, device)?;
            append_script(script, params, addition, values, suffix);
        }
        Ok(())
    }

    /// Prepare one atomic receipt batch from one history and head snapshot.
    pub(super) fn append_social_admissions_unlocked(
        &self,
        script: &mut String,
        params: &mut BTreeMap<String, DataValue>,
        roots: &[Share],
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        self.social_local_certificate(device.public())?;
        self.require_resolved_conversation_controls_unlocked()?;
        let history = self.conversation_control_history_unlocked()?;
        let controls = resolve_conversation_controls(&history)?;
        let parents: Vec<_> = self
            .private_state_heads(&conversation_control_key())?
            .into_iter()
            .map(|head| head.revision_id)
            .collect();
        let mut records = Vec::new();
        for root in roots {
            if let Some(operation) = self.social_admission_operation_unlocked(root, &controls)? {
                records.push(StateRevision::sign(
                    conversation_control_key(),
                    StateValue::ConversationControl(operation),
                    parents.clone(),
                    at,
                    device,
                )?);
            }
        }
        for (index, (addition, values)) in self
            .prepare_conversation_control_batch(&history, &records)?
            .into_iter()
            .enumerate()
        {
            append_script(script, params, addition, values, &format!("batch{index}"));
        }
        Ok(())
    }

    fn social_admission_operation_unlocked(
        &self,
        root: &Share,
        controls: &ConversationControls,
    ) -> Result<Option<ConversationControl>> {
        let scope = ThreadScope::from_share(root)?;
        if let Some(decision) = controls.conversations.get(&scope.share_id) {
            if decision.scope != scope {
                return Err(invalid("root differs from its private consent"));
            }
            return Ok(None);
        }
        // Legacy roots cannot take the ordinary admission path while adoption is pending.
        if !self
            .query_with_params(
                "?[scope] := *social_conversation_legacy{share_id:$id,scope}",
                BTreeMap::from([("id".into(), DataValue::from(root.id.as_str()))]),
            )?
            .rows
            .is_empty()
        {
            return Err(StoreError::ConversationUnavailable(
                ConversationParticipation::Pending,
            ));
        }
        let local = self
            .self_identity()?
            .ok_or(StoreError::IdentityUnbound)?
            .master_pubkey;
        let mut unfamiliar = false;
        if scope.recipients.len() > 1 {
            for person in scope.participants().into_iter().filter(|p| *p != local) {
                unfamiliar |= !self.social_contact(&person)?.is_some_and(|c| c.followed);
            }
        }
        let operation = if unfamiliar && !controls.allow_unfollowed_groups {
            ConversationControl::Decline { scope }
        } else {
            ConversationControl::Admit {
                scope,
                basis: if unfamiliar {
                    AdmissionBasis::IntroductionsAllowed
                } else {
                    AdmissionBasis::AllFollowed
                },
            }
        };
        Ok(Some(operation))
    }
}

fn append_script(
    script: &mut String,
    params: &mut BTreeMap<String, DataValue>,
    mut addition: String,
    values: BTreeMap<String, DataValue>,
    suffix: &str,
) {
    let mut names: Vec<_> = values.keys().collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for name in names {
        let renamed = format!("cc_{suffix}_{name}");
        addition = addition.replace(&format!("${name}"), &format!("${renamed}"));
        params.insert(renamed, values[name].clone());
    }
    script.push_str(&addition);
}
