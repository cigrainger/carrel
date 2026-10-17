//! Conversation policy and Cozo projections. Signed bytes remain authoritative.
use super::*;
use crate::private_library::PrivateReceive;
use carrel_core::conversation::{
    Avatar, ConversationContent as Content, ConversationRecord as Record, ThreadScope,
};

/// A local named selection of contacts, not a capability or mutable thread audience.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SocialGroup {
    /// Stable local selection identity.
    pub id: String,
    /// Reader-chosen label.
    pub name: String,
    /// Sorted followed master keys.
    pub members: Vec<PublicKey>,
}
/// A verified message and its author-removal state.
pub struct ConversationReply {
    /// Original signature and complete immutable content.
    pub record: Record,
    /// A verified author removal dominates this message.
    pub removed: bool,
}
/// Root, fixed audience and chronological retained replies.
pub struct Conversation {
    /// Original signed vouch and audience.
    pub share: Share,
    /// Local archived identity, which can differ from the sender's item ID.
    pub item_id: String,
    /// Chronological replies with permanent removal projections.
    pub replies: Vec<ConversationReply>,
    /// Root author has withdrawn the share.
    pub closed: bool,
}

/// A received conversation remains reachable after the vouch is withdrawn.
pub struct ReceivedConversation {
    /// Original root identity.
    pub id: String,
    /// Locally preferred author name.
    pub from: String,
    /// Original signed creation time.
    pub at_micros: i64,
    /// Permanent author withdrawal.
    pub closed: bool,
}

impl Store {
    /// Render the fixed audience using this desktop's private contact labels.
    pub fn social_audience_name(&self, share: &Share) -> Result<String> {
        share
            .recipients()
            .iter()
            .map(|key| self.social_name(key))
            .collect::<Result<Vec<_>>>()
            .map(|names| names.join(", "))
    }
    /// All received threads on one local article, including withdrawn vouches.
    pub fn received_item_conversations(&self, item_id: &str) -> Result<Vec<ReceivedConversation>> {
        let Some(binding) = self.self_identity()? else {
            return Ok(Vec::new());
        };
        let rows=self.query_with_params("?[key,created_at] := *social_receipt{share_id,item_id:$item}, *social_record{key,share_id,kind:'share',created_at} :sort -created_at,key",BTreeMap::from([("item".into(),DataValue::from(item_id))]))?;
        rows.rows
            .iter()
            .map(|row| {
                let Some(ShareFact::Share(root)) = self.social_record(&string(&row[0])?)? else {
                    return Err(invalid("missing received conversation"));
                };
                if !root.recipients().contains(&binding.master_pubkey) {
                    return Err(invalid("conversation is outside the bound identity"));
                }
                Ok(ReceivedConversation {
                    id: root.id.clone(),
                    from: self.social_name(&root.certificate.master_pubkey)?,
                    at_micros: root.created_at_micros,
                    closed: self.social_share_retracted(
                        &root.id,
                        &root.certificate.master_pubkey,
                        &root.recipient,
                    )?,
                })
            })
            .collect()
    }
    /// Read named groups; members remain exact public keys.
    pub fn social_groups(&self) -> Result<Vec<SocialGroup>> {
        self.query("?[id,name,members] := *social_group{id,name,members} :sort name,id")?
            .rows
            .iter()
            .map(|row| {
                let DataValue::List(members) = &row[2] else {
                    return Err(invalid("invalid group members"));
                };
                Ok(SocialGroup {
                    id: string(&row[0])?,
                    name: string(&row[1])?,
                    members: members.iter().map(public_key).collect::<Result<_>>()?,
                })
            })
            .collect()
    }
    /// Explicitly save a bounded contact selection. Existing conversations never change.
    pub fn save_social_group(
        &self,
        id: &str,
        name: &str,
        mut members: Vec<PublicKey>,
    ) -> Result<()> {
        let _social = self.social_lock.lock().expect("group write lock");
        let local = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            || name.trim().is_empty()
            || name.len() > 256
        {
            return Err(invalid("invalid group name or identity"));
        }
        members.sort();
        carrel_core::conversation::validate_recipients(&members, local.master_pubkey)?;
        for member in &members {
            self.require_social_contact(member)?;
        }
        self.query_with_params(
            "?[id,name,members] <- [[$id,$name,$members]] :put social_group {id=>name,members}",
            BTreeMap::from([
                ("id".into(), DataValue::from(id)),
                ("name".into(), DataValue::from(name.trim())),
                (
                    "members".into(),
                    DataValue::List(members.into_iter().map(key_value).collect()),
                ),
            ]),
        )?;
        Ok(())
    }
    /// Remove a selection, preserving every share made to it.
    pub fn remove_social_group(&self, id: &str) -> Result<()> {
        self.query_with_params(
            "?[id] <- [[$id]] :rm social_group {id}",
            BTreeMap::from([("id".into(), DataValue::from(id))]),
        )?;
        Ok(())
    }
    /// Resolve a contact label without letting a remote profile override a local alias.
    pub fn social_name(&self, master: &PublicKey) -> Result<String> {
        if let Some(contact) = self.social_contact(master)?
            && !contact.name.is_empty()
        {
            return Ok(contact.name);
        }
        if let Some(record) = self.social_profile(master)?
            && let Content::Profile { name, .. } = record.content
            && !name.is_empty()
        {
            return Ok(name);
        }
        Ok(master.to_hex()[..12].into())
    }
    /// Current verified presentation by generation and digest, independent of clocks.
    pub fn social_profile(&self, master: &PublicKey) -> Result<Option<Record>> {
        let rows = self.query_with_params(
            "?[key] := *social_record{key,author:$author,kind:'profile'}",
            BTreeMap::from([("author".into(), key_value(*master))]),
        )?;
        let mut current: Option<(u64, String, String, Record)> = None;
        for row in rows.rows {
            let key = string(&row[0])?;
            if let Some(ShareFact::Conversation(record)) = self.social_record(&key)?
                && let Content::Profile { generation, .. } = record.content
            {
                let revision = record
                    .profile_revision_id()?
                    .ok_or_else(|| invalid("missing profile presentation"))?;
                if current
                    .as_ref()
                    .is_none_or(|(known, digest, known_key, _)| {
                        (generation, &revision, &key) > (*known, digest, known_key)
                    })
                {
                    current = Some((generation, revision, key, *record));
                }
            }
        }
        Ok(current.map(|(_, _, _, record)| record))
    }
    /// Save self presentation and recipient-scoped copies atomically for all current follows.
    pub fn save_social_profile(
        &self,
        name: &str,
        avatar: Option<Avatar>,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let _identity = self.identity_lock.lock().expect("profile identity lock");
        let _social = self.social_lock.lock().expect("profile write lock");
        let certificate = self.social_local_certificate(device.public())?;
        let generation = self
            .social_profile(&certificate.master_pubkey)?
            .and_then(|r| {
                if let Content::Profile { generation, .. } = r.content {
                    Some(generation)
                } else {
                    None
                }
            })
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("profile generation exhausted"))?;
        let mut recipients: Vec<_> = self
            .social_contacts()?
            .into_iter()
            .filter(|c| c.followed)
            .map(|c| c.master)
            .collect();
        recipients.push(certificate.master_pubkey);
        let mut script = String::new();
        let mut params = BTreeMap::new();
        // Each statement has its own named parameters, allowing one atomic multi-recipient write.
        for (index, recipient) in recipients.into_iter().enumerate() {
            let fact = ShareFact::Conversation(Box::new(Record::sign(
                Content::Profile {
                    recipient,
                    generation,
                    name: name.trim().into(),
                    avatar: avatar.clone(),
                },
                at,
                certificate.clone(),
                device,
            )?));
            let (statement, values) = numbered_record(&fact, index)?;
            script.push_str(&statement);
            params.extend(values);
        }
        self.query_with_params(&script, params)?;
        self.social_changed();
        Ok(())
    }
    /// Send the latest profile to a newly followed contact without changing its generation.
    pub fn publish_social_profile_to(
        &self,
        recipient: PublicKey,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let _identity = self.identity_lock.lock().expect("profile identity lock");
        let _social = self.social_lock.lock().expect("profile write lock");
        let certificate = self.social_local_certificate(device.public())?;
        self.require_social_contact(&recipient)?;
        if let Some(current) = self.social_profile(&certificate.master_pubkey)? {
            let revision = current.profile_revision_id()?;
            let rows = self.query_with_params(
                "?[key] := *social_record{key,author:$author,recipient:$recipient,kind:'profile'}",
                BTreeMap::from([
                    ("author".into(), key_value(certificate.master_pubkey)),
                    ("recipient".into(), key_value(recipient)),
                ]),
            )?;
            for row in rows.rows {
                if let Some(ShareFact::Conversation(known)) =
                    self.social_record(&string(&row[0])?)?
                    && known.profile_revision_id()? == revision
                {
                    return Ok(());
                }
            }
            let Content::Profile {
                generation,
                name,
                avatar,
                ..
            } = current.content
            else {
                return Err(invalid("invalid current profile"));
            };
            let fact = ShareFact::Conversation(Box::new(Record::sign(
                Content::Profile {
                    recipient,
                    generation,
                    name,
                    avatar,
                },
                at,
                certificate,
                device,
            )?));
            self.query_with_params(PUT_RECORD, record_params(&fact)?)?;
        }
        Ok(())
    }
    /// Idempotently deliver an own-device profile update to this desktop's follows.
    pub fn publish_current_social_profiles(&self, device: &Keypair) -> Result<()> {
        for contact in self
            .social_contacts()?
            .into_iter()
            .filter(|contact| contact.followed)
        {
            self.publish_social_profile_to(
                contact.master,
                (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64,
                device,
            )?;
        }
        Ok(())
    }
    /// Read a participant's conversation. Removed bodies remain history, never presentation.
    pub fn social_conversation(&self, id: &str) -> Result<Conversation> {
        let share = self.conversation_root(id)?;
        let scope = ThreadScope::from_share(&share)?;
        let local = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if !scope.contains(&local.master_pubkey) {
            return Err(invalid("conversation is private to its participants"));
        }
        let rows=self.query_with_params("?[key,created_at,id,parent] := *social_reply{id,key,share_id:$share,parent}, *social_record{key,created_at} :sort created_at,id",BTreeMap::from([("share".into(),DataValue::from(id))]))?;
        let mut replies = Vec::new();
        for row in rows.rows {
            let Some(ShareFact::Conversation(record)) = self.social_record(&string(&row[0])?)?
            else {
                return Err(invalid("missing reply proof"));
            };
            if record.scope() != Some(&scope) {
                return Err(invalid("reply scope mismatch"));
            }
            let reply_id = string(&row[2])?;
            if !matches!(&record.content,Content::Reply{id,parent,..} if *id==reply_id && optional(parent.as_deref())==row[3])
            {
                return Err(invalid(
                    "reply ancestry index differs from its original proof",
                ));
            }
            let removed =
                self.reply_removed(&scope, &reply_id, &record.certificate.master_pubkey)?;
            replies.push(ConversationReply {
                record: *record,
                removed,
            });
        }
        let closed = self.social_share_retracted(id, &scope.sharer, &share.recipient)?;
        let item_id = self
            .query_with_params(
                "?[item_id] := *social_receipt{share_id:$id,item_id}",
                BTreeMap::from([("id".into(), DataValue::from(id))]),
            )?
            .rows
            .first()
            .map(|row| string(&row[0]))
            .transpose()?
            .unwrap_or_else(|| share.article.id.clone());
        Ok(Conversation {
            share,
            item_id,
            replies,
            closed,
        })
    }
    /// Cozo's recursive ancestry, from the selected reply through its oldest parent.
    /// Returned original signed records supply provenance without trusting an unsigned graph.
    pub fn social_reply_ancestry(&self, share_id: &str, reply_id: &str) -> Result<Vec<Record>> {
        self.social_conversation(share_id)?;
        let rows=self.query_with_params("ancestor[id,depth] := *social_reply{id:$reply,share_id:$share}, id=$reply, depth=0\nancestor[parent,depth] := ancestor[id,d], *social_reply{id,share_id:$share,parent}, parent != null, depth=d+1\n?[key,depth] := ancestor[id,depth], *social_reply{id,key,share_id:$share} :sort depth",BTreeMap::from([("reply".into(),DataValue::from(reply_id)),("share".into(),DataValue::from(share_id))]))?;
        rows.rows
            .iter()
            .map(|row| match self.social_record(&string(&row[0])?)? {
                Some(ShareFact::Conversation(record)) => Ok(*record),
                _ => Err(invalid("missing ancestry proof")),
            })
            .collect()
    }
    /// Queue a local immutable reply. Retrying never duplicates or changes its original text.
    pub fn reply_to_social_share(
        &self,
        share_id: &str,
        token: &str,
        parent: Option<String>,
        body: &str,
        at: i64,
        device: &Keypair,
    ) -> Result<Record> {
        let _identity = self.identity_lock.lock().expect("reply identity lock");
        let _social = self.social_lock.lock().expect("reply write lock");
        let certificate = self.social_local_certificate(device.public())?;
        let scope = ThreadScope::from_share(&self.conversation_root(share_id)?)?;
        let id = Share::draft_id(&certificate, token)?;
        if let Some(ShareFact::Conversation(existing)) =
            self.social_record(&format!("reply/{id}"))?
        {
            if let Content::Reply {
                scope: known,
                parent: known_parent,
                body: known_body,
                ..
            } = &existing.content
                && *known == scope
                && *known_parent == parent
                && known_body == body
            {
                return Ok(*existing);
            }
            return Err(invalid("reply retry changed its original content"));
        }
        if self.social_conversation(share_id)?.closed {
            return Err(invalid("this share has been withdrawn"));
        }
        if let Some(parent) = &parent
            && self.social_record(&format!("reply/{parent}"))?.is_none()
        {
            return Err(invalid("reply parent is unavailable"));
        }
        let record = Record::sign(
            Content::Reply {
                scope,
                id,
                request_id: token.into(),
                parent,
                body: body.into(),
            },
            at,
            certificate,
            device,
        )?;
        self.accept_conversation_unlocked(&record, device.public(), true)?;
        Ok(record)
    }
    /// Append the author's permanent removal proof, retaining context and signatures.
    pub fn remove_social_reply(
        &self,
        share_id: &str,
        reply_id: &str,
        at: i64,
        device: &Keypair,
    ) -> Result<()> {
        let _identity = self
            .identity_lock
            .lock()
            .expect("reply removal identity lock");
        let _social = self.social_lock.lock().expect("reply removal lock");
        let certificate = self.social_local_certificate(device.public())?;
        let scope = ThreadScope::from_share(&self.conversation_root(share_id)?)?;
        if self.social_record(&format!("reply/{reply_id}"))?.is_none() {
            return Err(invalid("message is unavailable"));
        }
        let record = Record::sign(
            Content::Remove {
                scope,
                reply_id: reply_id.into(),
            },
            at,
            certificate,
            device,
        )?;
        self.accept_conversation_unlocked(&record, device.public(), true)?;
        Ok(())
    }
    pub(super) fn accept_conversation_unlocked(
        &self,
        record: &Record,
        sender: &PublicKey,
        from_self: bool,
    ) -> Result<SocialReceive> {
        record.verify()?;
        let fact = ShareFact::Conversation(Box::new(record.clone()));
        let key = fact.key()?;
        if let Some(existing) = self.social_record(&key)? {
            if existing == fact {
                return Ok(SocialReceive::Existing);
            }
            return Err(invalid("immutable conversation conflict"));
        }
        let local = self
            .self_identity()?
            .ok_or(StoreError::IdentityUnbound)?
            .master_pubkey;
        if let Content::Profile { recipient, .. } = &record.content {
            if (!from_self && (record.certificate.master_pubkey != *sender || *recipient != local))
                || (from_self && record.certificate.master_pubkey != local && *recipient != local)
            {
                return Err(invalid("profile is outside its signed audience"));
            }
        } else {
            let scope = record.scope().expect("thread record");
            if !scope.contains(&local) || (!from_self && !scope.contains(sender)) {
                return Err(invalid("conversation sender is not a participant"));
            }
            let Some(ShareFact::Share(root)) =
                self.social_record(&format!("share/{}", scope.share_id))?
            else {
                return self.stage_social_record(&fact);
            };
            if ThreadScope::from_share(&root)? != *scope {
                return Err(invalid("conversation audience differs from original vouch"));
            }
            let dependency = match &record.content {
                Content::Reply { parent, .. } => parent.as_deref(),
                Content::Remove { reply_id, .. } => Some(reply_id.as_str()),
                _ => None,
            };
            if let Some(id) = dependency {
                let Some(ShareFact::Conversation(parent)) =
                    self.social_record(&format!("reply/{id}"))?
                else {
                    return self.stage_social_record(&fact);
                };
                if parent.scope() != Some(scope)
                    || (matches!(record.content, Content::Remove { .. })
                        && parent.certificate.master_pubkey != record.certificate.master_pubkey)
                {
                    return Err(invalid("reply parent or removal author does not match"));
                }
            }
        }
        let mut params = record_params(&fact)?;
        let mut script = PUT_RECORD.to_owned();
        if let Content::Reply {
            scope, id, parent, ..
        } = &record.content
        {
            params.extend([
                ("reply".into(), DataValue::from(id.as_str())),
                ("parent".into(), optional(parent.as_deref())),
                ("thread".into(), DataValue::from(scope.share_id.as_str())),
            ]);
            script.push_str("{ ?[id,key,share_id,parent] <- [[$reply,$key,$thread,$parent]] :put social_reply {id=>key,share_id,parent} }");
        }
        script.push_str("{ ?[key] <- [[$key]] :rm social_pending {key} }");
        self.query_with_params(&script, params)?;
        self.social_changed();
        Ok(SocialReceive::Applied)
    }
    fn stage_social_record(&self, fact: &ShareFact) -> Result<SocialReceive> {
        if let ShareFact::Conversation(record) = fact
            && let Content::Reply {
                scope,
                id,
                parent: Some(parent),
                ..
            } = &record.content
        {
            let mut seen = std::collections::BTreeSet::from([id.clone()]);
            let mut next = Some(parent.clone());
            while let Some(parent) = next {
                if !seen.insert(parent.clone()) {
                    return Err(invalid("cyclic reply ancestry"));
                }
                let rows = self.query_with_params(
                    "?[encoded] := *social_pending{key:$key,encoded}",
                    BTreeMap::from([("key".into(), DataValue::from(format!("reply/{parent}")))]),
                )?;
                let Some(row) = rows.rows.first() else {
                    break;
                };
                let DataValue::Bytes(encoded) = &row[0] else {
                    return Err(invalid("invalid pending reply"));
                };
                let ShareFact::Conversation(pending) = ShareFact::from_bytes(encoded)? else {
                    return Err(invalid("invalid pending parent"));
                };
                let Content::Reply {
                    scope: known,
                    parent,
                    ..
                } = pending.content
                else {
                    return Err(invalid("invalid pending parent"));
                };
                if known != *scope {
                    return Err(invalid("pending parent belongs to another conversation"));
                }
                next = parent;
            }
        }
        let params = record_params(fact)?;
        let existing = self.query_with_params(
            "?[encoded] := *social_pending{key:$key,encoded}",
            params.clone(),
        )?;
        if let Some(row) = existing.rows.first() {
            if row[0] != params["encoded"] {
                return Err(invalid("immutable pending conversation conflict"));
            }
            return Ok(SocialReceive::Pending);
        }
        let rows =
            self.query("?[key,size] := *social_pending{key,encoded}, size=length(encoded)")?;
        let size = rows
            .rows
            .iter()
            .map(|row| integer(&row[1]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<i64>();
        if rows.rows.len() >= 512 || size + fact.to_bytes()?.len() as i64 > 32 * 1024 * 1024 {
            return Err(invalid("conversation dependency queue is full"));
        }
        self.query_with_params(
            "?[key,encoded] <- [[$key,$encoded]] :put social_pending {key=>encoded}",
            params,
        )?;
        Ok(SocialReceive::Pending)
    }
    /// Promote verified dependencies after root/parent arrival, in bounded passes.
    pub fn promote_pending_social(&self) -> Result<bool> {
        let _identity = self
            .identity_lock
            .lock()
            .expect("pending social identity lock");
        let _social = self.social_lock.lock().expect("pending social lock");
        let local = self
            .self_identity()?
            .ok_or(StoreError::IdentityUnbound)?
            .master_pubkey;
        let mut changed = false;
        for _ in 0..16 {
            let rows =
                self.query("?[key,encoded] := *social_pending{key,encoded} :sort key :limit 512")?;
            let mut progressed = false;
            for row in rows.rows {
                let DataValue::Bytes(bytes) = &row[1] else {
                    return Err(invalid("invalid pending conversation"));
                };
                let fact = ShareFact::from_bytes(bytes)?;
                if let ShareFact::Conversation(record) = fact {
                    match self.accept_conversation_unlocked(&record, &local, true) {
                        Ok(SocialReceive::Applied | SocialReceive::Existing) => {
                            progressed = true;
                            changed = true;
                        }
                        Ok(_) => {}
                        Err(StoreError::InvalidInput { .. } | StoreError::Share(_)) => {
                            self.query_with_params(
                                "?[key] <- [[$key]] :rm social_pending {key}",
                                BTreeMap::from([("key".into(), row[0].clone())]),
                            )?;
                            progressed = true;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            if !progressed {
                break;
            }
        }
        Ok(changed)
    }
    /// Self transport preserves original external proofs and checks the live forwarding device.
    pub fn accept_self_social_record(
        &self,
        fact: &ShareFact,
        sender: &PublicKey,
    ) -> Result<PrivateReceive> {
        let author = fact.author();
        Ok(
            match self.accept_social_record_from(
                fact,
                &author,
                (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64,
                Some(sender),
            )? {
                SocialReceive::Applied => PrivateReceive::Applied,
                SocialReceive::Existing => PrivateReceive::Existing,
                SocialReceive::Pending => PrivateReceive::Pending,
                SocialReceive::Retracted => PrivateReceive::Removed,
            },
        )
    }
    fn conversation_root(&self, id: &str) -> Result<Share> {
        match self.social_record(&format!("share/{id}"))? {
            Some(ShareFact::Share(root)) => Ok(*root),
            _ => Err(invalid("conversation is unavailable")),
        }
    }
    fn reply_removed(&self, scope: &ThreadScope, id: &str, author: &PublicKey) -> Result<bool> {
        let rows = self.query_with_params(
            "?[key] := *social_record{key,share_id:$share,author:$author,kind:'reply_removal'}",
            BTreeMap::from([
                ("share".into(), DataValue::from(scope.share_id.as_str())),
                ("author".into(), key_value(*author)),
            ]),
        )?;
        for row in rows.rows {
            if let Some(ShareFact::Conversation(record)) = self.social_record(&string(&row[0])?)?
                && let Content::Remove { reply_id, .. } = record.content
                && reply_id == id
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn social_changed(&self) {
        self.social_revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }
}
fn numbered_record(
    fact: &ShareFact,
    index: usize,
) -> Result<(String, BTreeMap<String, DataValue>)> {
    let mut script = PUT_RECORD.to_owned();
    let params = record_params(fact)?;
    let mut output = BTreeMap::new();
    // Longest names first avoids replacing a prefix of another parameter.
    let mut names: Vec<_> = params.keys().cloned().collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for name in names {
        let numbered = format!("{name}_{index}");
        script = script.replace(&format!("${name}"), &format!("${numbered}"));
        output.insert(numbered, params[&name].clone());
    }
    Ok((script, output))
}
