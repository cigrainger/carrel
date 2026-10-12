//! Typed recipient-scoped social history. Contacts never authorize self protocols.
use crate::{Result, Store, StoreError};
use carrel_core::identity::{DeviceCert, Keypair, PublicKey};
use carrel_core::private_library::{BlobReference, SavedItem};
use carrel_core::sharing::{
    ContactCard, Share, ShareFact, ShareRetraction, public_article_snapshot,
};
use cozo::{DataValue, JsonData, Validity};
use std::collections::BTreeMap;

/// A manually pinned person, with optional signed connection information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SocialContact {
    /// Stable master public key.
    pub master: PublicKey,
    /// Private local name.
    pub name: String,
    /// Public contact code, without private document capabilities.
    pub card: Option<ContactCard>,
    /// Whether new exchange is enabled.
    pub followed: bool,
}
/// Native request to save an explicit vouch; no snapshot comes from the webview.
pub struct ShareDraft {
    /// Stable UI draft token, retained on retry.
    pub request_id: String,
    /// Explicitly selected person.
    pub recipient: PublicKey,
    /// Current archive prepared by the native engine.
    pub article: SavedItem,
    /// Deliberately entered optional note.
    pub note: Option<String>,
}
/// Durable receipt result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocialReceive {
    /// New signed record and local projections committed together.
    Applied,
    /// Identical immutable record was already present.
    Existing,
    /// A retraction dominates this creation.
    Retracted,
}
/// A bounded cache download that must be authorized against its original share.
#[derive(Clone, Debug)]
pub struct PendingShareBlob {
    /// Signed share granting access.
    pub share_id: String,
    /// Exact digest and size.
    pub reference: BlobReference,
    /// Readable body before optional images.
    pub is_body: bool,
}
/// A local namespace binding, never distributed to friends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SocialDocument {
    /// SDK namespace ID.
    pub namespace: [u8; 32],
    /// Private serialized SDK capability.
    pub capability: Vec<u8>,
}
/// One preserved vouch and the recipient's own stable article identity.
#[derive(Clone, Debug)]
pub struct ReceivedShare {
    /// Original signed vouch.
    pub share: Share,
    /// Existing or newly archived local item ID.
    pub item_id: String,
    /// User's local name for the author.
    pub name: String,
}

/// A sender's own vouch, including an explicit withdrawal state.
pub struct SentShare {
    /// Original signed share.
    pub share: Share,
    /// Local recipient name.
    pub name: String,
    /// Whether the author has appended a retraction.
    pub retracted: bool,
}

const PUT_RECORD: &str = r#"{
    ?[key, encoded, author, recipient, share_id, kind, created_at] <- [[$key,$encoded,$author,$recipient,$share_id,$kind,$created]]
    :put social_record {key => encoded, author, recipient, share_id, kind, created_at}
}"#;

impl Store {
    /// A quiet UI invalidation generation, with no facts or user-visible counts.
    pub fn social_revision(&self) -> u64 {
        self.social_revision
            .load(std::sync::atomic::Ordering::Acquire)
    }
    /// List local contact settings in stable name/key order.
    pub fn social_contacts(&self) -> Result<Vec<SocialContact>> {
        let rows=self.query("?[master,name,card,followed] := *social_contact{master,name,card,followed} :sort name, master")?;
        rows.rows
            .iter()
            .map(|row| {
                let master = public_key(&row[0])?;
                let card = match &row[2] {
                    DataValue::Null => None,
                    DataValue::Bytes(bytes) => Some(ContactCard::from_text(
                        std::str::from_utf8(bytes)
                            .map_err(|_| invalid("invalid contact encoding"))?,
                    )?),
                    _ => return Err(invalid("invalid contact card")),
                };
                if card
                    .as_ref()
                    .is_some_and(|card| card.certificate.master_pubkey != master)
                {
                    return Err(invalid("contact identity mismatch"));
                }
                Ok(SocialContact {
                    master,
                    name: string(&row[1])?,
                    card,
                    followed: boolean(&row[3])?,
                })
            })
            .collect()
    }
    /// Resolve a manually configured master, never an incoming peer claim.
    pub fn social_contact(&self, master: &PublicKey) -> Result<Option<SocialContact>> {
        Ok(self
            .social_contacts()?
            .into_iter()
            .find(|contact| contact.master == *master))
    }
    /// Pin a public key explicitly, preserving a stored card when only a key is supplied.
    pub fn follow_social_contact(
        &self,
        master: PublicKey,
        name: &str,
        card: Option<&ContactCard>,
    ) -> Result<SocialContact> {
        let _guard = self.social_lock.lock().expect("social contact lock");
        master
            .validate()
            .map_err(|_| invalid("invalid person's public key"))?;
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if binding.master_pubkey == master {
            return Err(invalid("choose another person's public key"));
        }
        let name = name.trim();
        if name.len() > 256 {
            return Err(invalid("person's name is too long"));
        }
        if let Some(card) = card {
            card.verify()?;
            if card.certificate.master_pubkey != master {
                return Err(invalid("contact belongs to another public key"));
            }
        }
        let existing = self.social_contact(&master)?;
        let name = if name.is_empty() {
            existing
                .as_ref()
                .map(|contact| contact.name.clone())
                .unwrap_or_else(|| master.to_hex()[..12].to_owned())
        } else {
            name.into()
        };
        let card = card
            .cloned()
            .or_else(|| existing.and_then(|contact| contact.card));
        self.put_contact(&SocialContact {
            master,
            name,
            card,
            followed: true,
        })?;
        self.social_contact(&master)?
            .ok_or_else(|| invalid("contact was not saved"))
    }
    /// Stop future requests and downloads while preserving accepted articles and facts.
    pub fn unfollow_social_contact(&self, master: &PublicKey) -> Result<bool> {
        let _guard = self.social_lock.lock().expect("social contact lock");
        let Some(mut contact) = self.social_contact(master)? else {
            return Ok(false);
        };
        contact.followed = false;
        self.put_contact(&contact)?;
        Ok(true)
    }
    /// Complete connection information only for an already manually pinned master.
    /// A known device may refresh its address; another device requires explicit import.
    pub fn complete_social_contact(&self, card: &ContactCard) -> Result<()> {
        card.verify()?;
        let _guard = self.social_lock.lock().expect("social contact lock");
        let Some(mut contact) = self.social_contact(&card.certificate.master_pubkey)? else {
            return Err(invalid("person is not followed"));
        };
        if !contact.followed
            || contact.card.as_ref().is_some_and(|known| {
                known.certificate.device_pubkey != card.certificate.device_pubkey
            })
        {
            return Err(invalid("contact device is not authorized"));
        }
        contact.card = Some(card.clone());
        self.put_contact(&contact)
    }
    fn put_contact(&self, contact: &SocialContact) -> Result<()> {
        self.query_with_params("?[master,name,card,followed] <- [[$master,$name,$card,$followed]] :put social_contact {master => name,card,followed}",BTreeMap::from([
            ("master".into(),key_value(contact.master)),("name".into(),DataValue::from(contact.name.as_str())),("card".into(),contact.card.as_ref().map(|card|card.to_text().map(|text|DataValue::Bytes(text.into_bytes()))).transpose()?.unwrap_or(DataValue::Null)),("followed".into(),DataValue::Bool(contact.followed)),
        ]))?;
        Ok(())
    }
    /// The active local certificate used to sign vouches and authenticate requests.
    pub fn social_local_certificate(&self, device: &PublicKey) -> Result<DeviceCert> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if binding.device_pubkey != *device {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.self_devices()?
            .into_iter()
            .find(|row| row.certificate.device_pubkey == *device && row.is_authorized())
            .map(|row| row.certificate)
            .ok_or(StoreError::IdentityDeviceUnauthorized)
    }
    /// Save a signed vouch atomically. Repeating a draft cannot send it twice.
    pub fn create_social_share(
        &self,
        draft: ShareDraft,
        at_micros: i64,
        device: &Keypair,
    ) -> Result<Share> {
        let _private = self.private_state_lock.lock().expect("share archive lock");
        let _identity = self.identity_lock.lock().expect("share identity lock");
        let _social = self.social_lock.lock().expect("share write lock");
        let certificate = self.social_local_certificate(device.public())?;
        self.require_social_contact(&draft.recipient)?;
        let id = Share::draft_id(&certificate, &draft.request_id)?;
        if let Some(ShareFact::Share(existing)) = self.social_record(&format!("share/{id}"))? {
            if existing.recipient != draft.recipient
                || existing.article.id != draft.article.id
                || existing.note != draft.note
            {
                return Err(invalid(
                    "this share draft was already saved with different content",
                ));
            }
            return Ok(*existing);
        }
        let current = self
            .saved_item_snapshot_unlocked(&draft.article.id)?
            .ok_or_else(|| invalid("article is no longer available"))?;
        if current.current_images.as_ref().is_some_and(|scope| {
            draft
                .article
                .resources
                .iter()
                .any(|image| !scope.contains(image))
        }) {
            return Err(invalid(
                "an image is outside this article's approved cache scope",
            ));
        }
        let current = public_article_snapshot(current, draft.article.resources.clone())?;
        if current != draft.article {
            return Err(invalid(
                "article changed while preparing the share; try again",
            ));
        }
        let share = Share::sign(
            draft.request_id,
            draft.recipient,
            draft.article,
            draft.note,
            at_micros,
            certificate,
            device,
        )?;
        self.query_with_params(
            PUT_RECORD,
            record_params(&ShareFact::Share(Box::new(share.clone())))?,
        )?;
        Ok(share)
    }
    /// Retract only this master's own vouch; an older timestamp still withdraws it.
    pub fn retract_social_share(&self, id: &str, at_micros: i64, device: &Keypair) -> Result<()> {
        let _identity = self.identity_lock.lock().expect("share identity lock");
        let _social = self.social_lock.lock().expect("share retraction lock");
        let certificate = self.social_local_certificate(device.public())?;
        let Some(ShareFact::Share(share)) = self.social_record(&format!("share/{id}"))? else {
            return Err(invalid("share is unavailable"));
        };
        if share.certificate.master_pubkey != certificate.master_pubkey {
            return Err(invalid("only the author can retract a share"));
        }
        let record = ShareFact::Retraction(Box::new(ShareRetraction::sign(
            id.into(),
            share.recipient,
            at_micros,
            certificate,
            device,
        )?));
        self.query_with_params(PUT_RECORD, record_params(&record)?)?;
        Ok(())
    }
    /// Read immutable bytes and check that all query indexes derive from their proof.
    pub fn social_record(&self, key: &str) -> Result<Option<ShareFact>> {
        let rows=self.query_with_params("?[key,encoded,author,recipient,share_id,kind,created_at] := *social_record{key,encoded,author,recipient,share_id,kind,created_at}, key=$key",BTreeMap::from([("key".into(),DataValue::from(key))]))?;
        rows.rows
            .first()
            .map(|row| {
                let DataValue::Bytes(bytes) = &row[1] else {
                    return Err(invalid("invalid social record encoding"));
                };
                let fact = ShareFact::from_bytes(bytes)?;
                let expected = record_params(&fact)?;
                for (column, param) in [
                    (0, "key"),
                    (1, "encoded"),
                    (2, "author"),
                    (3, "recipient"),
                    (4, "share_id"),
                    (5, "kind"),
                    (6, "created"),
                ] {
                    if row[column] != expected[param] {
                        return Err(invalid(
                            "social record index does not match its signed content",
                        ));
                    }
                }
                Ok(fact)
            })
            .transpose()
    }
    fn require_social_contact(&self, master: &PublicKey) -> Result<SocialContact> {
        self.social_contact(master)?
            .filter(|contact| contact.followed)
            .ok_or_else(|| invalid("person is not followed"))
    }
    /// Own signed history for the separate mirror, with an exclusive bounded key.
    pub fn social_history(&self, after: Option<&str>, limit: usize) -> Result<Vec<ShareFact>> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.social_local_certificate(&binding.device_pubkey)?;
        let rows = self.query_with_params(
            "?[key] := *social_record{key,author:$author}, key>$after :sort key :limit $limit",
            BTreeMap::from([
                ("author".into(), key_value(binding.master_pubkey)),
                ("after".into(), DataValue::from(after.unwrap_or(""))),
                ("limit".into(), DataValue::from(limit.clamp(1, 100) as i64)),
            ]),
        )?;
        rows.rows
            .iter()
            .map(|row| {
                self.social_record(&string(&row[0])?)?
                    .ok_or_else(|| invalid("missing share history"))
            })
            .collect()
    }
    /// Verify that a recipient remains explicitly permitted before exposing anything.
    pub fn authorize_social_recipient(&self, master: &PublicKey) -> Result<()> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.social_local_certificate(&binding.device_pubkey)?;
        self.require_social_contact(master)?;
        Ok(())
    }
    /// Resolve a digest only within this recipient's unretracted outgoing share.
    pub fn authorized_social_blob(
        &self,
        recipient: &PublicKey,
        share_id: &str,
        blob_id: &str,
    ) -> Result<BlobReference> {
        self.authorize_social_recipient(recipient)?;
        let Some(ShareFact::Share(share)) = self.social_record(&format!("share/{share_id}"))?
        else {
            return Err(invalid("shared content is unavailable"));
        };
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if share.recipient != *recipient
            || share.certificate.master_pubkey != binding.master_pubkey
            || self.social_share_retracted(
                &share.id,
                &share.certificate.master_pubkey,
                &share.recipient,
            )?
        {
            return Err(invalid("shared content is unavailable"));
        }
        share
            .article
            .blobs()
            .into_iter()
            .find(|reference| reference.blob_id == blob_id)
            .ok_or_else(|| invalid("content is not part of this share"))
    }
    fn social_share_retracted(
        &self,
        id: &str,
        author: &PublicKey,
        recipient: &PublicKey,
    ) -> Result<bool> {
        let rows=self.query_with_params("?[key] := *social_record{key,share_id:$id,author:$author,recipient:$recipient,kind:'retraction'} :limit 1",BTreeMap::from([("id".into(),DataValue::from(id)),("author".into(),key_value(*author)),("recipient".into(),key_value(*recipient))]))?;
        if let Some(row) = rows.rows.first() {
            self.social_record(&string(&row[0])?)?;
            return Ok(true);
        }
        Ok(false)
    }
    /// Accept only a manually followed author and the local master as recipient.
    /// Article metadata, receipt and pending content commit in one transaction.
    pub fn accept_social_record(
        &self,
        fact: &ShareFact,
        expected_author: &PublicKey,
        at_micros: i64,
    ) -> Result<SocialReceive> {
        fact.verify()?;
        let _private = self.private_state_lock.lock().expect("shared article lock");
        let _identity = self
            .identity_lock
            .lock()
            .expect("share receipt identity lock");
        let _social = self.social_lock.lock().expect("share receipt lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.social_local_certificate(&binding.device_pubkey)?;
        self.require_social_contact(expected_author)?;
        if fact.author() != *expected_author || fact.recipient() != binding.master_pubkey {
            return Err(invalid(
                "share does not belong to this author and recipient",
            ));
        }
        if let Some(existing) = self.social_record(&fact.key()?)? {
            if existing != *fact {
                return Err(invalid("immutable share record conflict"));
            }
            return Ok(SocialReceive::Existing);
        }
        let mut params = record_params(fact)?;
        let mut script = PUT_RECORD.to_owned();
        let result = match fact {
            ShareFact::Retraction(retraction) => {
                if let Some(ShareFact::Share(existing)) =
                    self.social_record(&format!("share/{}", retraction.share_id))?
                    && (existing.recipient != retraction.recipient
                        || existing.certificate.master_pubkey
                            != retraction.certificate.master_pubkey)
                {
                    return Err(invalid("retraction belongs to another author's share"));
                }
                script.push_str("\n{ ?[share_id,blob_id] := *social_blob_pending{share_id:$share_id,blob_id}, share_id=$share_id :rm social_blob_pending {share_id,blob_id} }");
                SocialReceive::Applied
            }
            ShareFact::Share(share) => {
                if self.social_share_retracted(
                    &share.id,
                    &share.certificate.master_pubkey,
                    &share.recipient,
                )? {
                    SocialReceive::Retracted
                } else {
                    let (materialize, archive_params) =
                        self.prepare_shared_article(&share.article, at_micros)?;
                    params.extend(archive_params);
                    script.push_str(&materialize);
                    script.push_str("\n{ ?[share_id,item_id] <- [[$share_id,$item_id]] :put social_receipt {share_id => item_id} }");
                    let references =
                        share
                            .article
                            .blobs()
                            .into_iter()
                            .map(|reference| {
                                DataValue::List(vec![
                                    DataValue::from(reference.blob_id.as_str()),
                                    DataValue::from(reference.byte_size as i64),
                                    DataValue::Bool(
                                        share.article.contents.iter().any(|content| {
                                            content.blob.blob_id == reference.blob_id
                                        }),
                                    ),
                                ])
                            })
                            .collect();
                    params.insert("references".into(), DataValue::List(references));
                    script.push_str("\n{ refs[blob_id,byte_size,is_body] <- $references ?[share_id,blob_id,byte_size,is_body] := refs[blob_id,byte_size,is_body], share_id=$share_id :put social_blob_pending {share_id,blob_id => byte_size,is_body} }");
                    SocialReceive::Applied
                }
            }
        };
        self.query_with_params(&script, params)?;
        if result == SocialReceive::Applied {
            self.social_revision
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        Ok(result)
    }
    fn prepare_shared_article(
        &self,
        item: &SavedItem,
        at_micros: i64,
    ) -> Result<(String, BTreeMap<String, DataValue>)> {
        let raw = item
            .primary_url
            .as_deref()
            .ok_or_else(|| invalid("shared article has no address"))?;
        let parsed = url::Url::parse(raw).map_err(|source| StoreError::InvalidUrl {
            url: raw.into(),
            source,
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(invalid("invalid shared article address"));
        }
        let url = crate::ids::canonicalize_external_identifier(raw);
        let existing = self.article_by_urls_unlocked(std::slice::from_ref(&url))?;
        let matched = existing.is_some();
        let id = existing.unwrap_or_else(|| crate::ids::id_for_external(&url));
        let local = self.get_item_detail(&id)?;
        if !matched && local.is_some() {
            return Err(invalid(
                "shared address conflicts with an existing article identity",
            ));
        }
        let fill = local.as_ref().is_none_or(|item| item.readable.is_none());
        let article = item.article.as_ref();
        let content = item.contents.first();
        let params = BTreeMap::from([
            ("item_id".into(), DataValue::from(id.as_str())),
            ("url".into(), DataValue::from(url)),
            ("title".into(), DataValue::from(item.title.as_str())),
            (
                "creators".into(),
                DataValue::List(
                    item.creators
                        .iter()
                        .map(|creator| DataValue::from(creator.as_str()))
                        .collect(),
                ),
            ),
            (
                "published".into(),
                item.published_at_micros
                    .map(validity)
                    .unwrap_or(DataValue::Null),
            ),
            ("language".into(), optional(item.language.as_deref())),
            ("summary".into(), optional(item.summary.as_deref())),
            ("at".into(), validity(at_micros)),
            (
                "site".into(),
                optional(article.and_then(|meta| meta.site_name.as_deref())),
            ),
            (
                "byline".into(),
                optional(article.and_then(|meta| meta.byline.as_deref())),
            ),
            (
                "words".into(),
                article
                    .and_then(|meta| meta.word_count)
                    .map(DataValue::from)
                    .unwrap_or(DataValue::Null),
            ),
            (
                "minutes".into(),
                article
                    .and_then(|meta| meta.estimated_read_minutes)
                    .map(DataValue::from)
                    .unwrap_or(DataValue::Null),
            ),
            ("fill".into(), DataValue::Bool(fill)),
            ("has_body".into(), DataValue::Bool(content.is_some())),
            (
                "body".into(),
                optional(content.map(|body| body.blob.blob_id.as_str())),
            ),
            (
                "body_size".into(),
                content
                    .map(|body| DataValue::from(body.blob.byte_size as i64))
                    .unwrap_or(DataValue::Null),
            ),
            (
                "extractor".into(),
                optional(content.and_then(|body| body.extracted_with.as_deref())),
            ),
        ]);
        let script = r#"
        {
            ?[id,kind,title,creators,primary_url,published_at,language,summary,discovered_at] :=
                not *item{id:$item_id}, id=$item_id, kind='article', title=$title, creators=$creators,
                primary_url=$url,published_at=$published,language=$language,summary=$summary,discovered_at=$at
            :put item {id => kind,title,creators,primary_url,published_at,language,summary,discovered_at}
        }
        {
            ?[item_id,scheme,value,is_canonical,discovered_at] := not *item_identifier{item_id:$item_id,scheme:'url',value:$url},
                item_id=$item_id,scheme='url',value=$url,is_canonical=true,discovered_at=$at
            :put item_identifier {item_id,scheme,value => is_canonical,discovered_at}
        }
        {
            ?[item_id,feed_url,word_count,estimated_read_minutes,site_name,byline] :=
                not *item_article{item_id:$item_id}, item_id=$item_id,feed_url=null,
                word_count=$words,estimated_read_minutes=$minutes,site_name=$site,byline=$byline
            :put item_article {item_id => feed_url,word_count,estimated_read_minutes,site_name,byline}
        }
        "#;
        Ok((script.into(), params))
    }
    /// Bounded pending content for one followed author, readable body first.
    pub fn pending_social_blobs(
        &self,
        author: &PublicKey,
        limit: usize,
    ) -> Result<Vec<PendingShareBlob>> {
        self.pending_social_blobs_after(author, limit, None)
    }
    /// Continue the body-first recovery queue so an unavailable blob cannot starve others.
    pub fn pending_social_blobs_after(
        &self,
        author: &PublicKey,
        limit: usize,
        after: Option<&PendingShareBlob>,
    ) -> Result<Vec<PendingShareBlob>> {
        self.require_social_contact(author)?;
        let rows=self.query_with_params("?[share_id,blob_id,byte_size,is_body] := *social_blob_pending{share_id,blob_id,byte_size,is_body}, *social_record{share_id,kind:'share',author:$author}, (!$has_after || (is_body == false && $body == true) || (is_body == $body && (share_id > $share || (share_id == $share && blob_id > $blob)))) :sort -is_body, share_id, blob_id :limit $limit",BTreeMap::from([("author".into(),key_value(*author)),("limit".into(),DataValue::from(limit.clamp(1,100) as i64)),("has_after".into(),DataValue::Bool(after.is_some())),("body".into(),DataValue::Bool(after.is_some_and(|blob| blob.is_body))),("share".into(),DataValue::from(after.map_or("",|blob|blob.share_id.as_str()))),("blob".into(),DataValue::from(after.map_or("",|blob|blob.reference.blob_id.as_str())))]))?;
        rows.rows
            .iter()
            .map(|row| {
                let id = string(&row[0])?;
                let blob = string(&row[1])?;
                let Some(ShareFact::Share(share)) = self.social_record(&format!("share/{id}"))?
                else {
                    return Err(invalid("missing pending share"));
                };
                let expected = share
                    .article
                    .blobs()
                    .into_iter()
                    .find(|reference| reference.blob_id == blob)
                    .ok_or_else(|| invalid("pending content is not signed into its share"))?;
                if DataValue::from(expected.byte_size as i64) != row[2] {
                    return Err(invalid("pending content size differs from its proof"));
                }
                Ok(PendingShareBlob {
                    share_id: id,
                    reference: expected,
                    is_body: boolean(&row[3])?,
                })
            })
            .collect()
    }
    /// A verified cache insertion completed; remove only that exact pending reference.
    pub fn complete_social_blob(&self, share_id: &str, blob_id: &str) -> Result<()> {
        let _private = self.private_state_lock.lock().expect("shared content lock");
        let _social = self
            .social_lock
            .lock()
            .expect("shared content receipt lock");
        if self
            .query_with_params(
                "?[id] := *social_blob_pending{share_id:$id,blob_id:$blob}, id=$id",
                BTreeMap::from([
                    ("id".into(), DataValue::from(share_id)),
                    ("blob".into(), DataValue::from(blob_id)),
                ]),
            )?
            .rows
            .is_empty()
        {
            return Ok(());
        }
        let Some(ShareFact::Share(share)) = self.social_record(&format!("share/{share_id}"))?
        else {
            return Err(invalid("missing shared content proof"));
        };
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        if share.recipient != binding.master_pubkey {
            return Err(invalid("shared content belongs to another recipient"));
        }
        let item_id = self
            .query_with_params(
                "?[item_id] := *social_receipt{share_id:$id,item_id}",
                BTreeMap::from([("id".into(), DataValue::from(share_id))]),
            )?
            .rows
            .first()
            .map(|row| string(&row[0]))
            .transpose()?
            .ok_or_else(|| invalid("missing shared article receipt"))?;
        let reference = share
            .article
            .blobs()
            .into_iter()
            .find(|reference| reference.blob_id == blob_id)
            .ok_or_else(|| invalid("content is outside its share"))?;
        let mut params = BTreeMap::from([
            ("id".into(), DataValue::from(share_id)),
            ("blob".into(), DataValue::from(blob_id)),
            ("item".into(), DataValue::from(item_id.as_str())),
        ]);
        let mut script = String::new();
        if let Some(body) = share.article.contents.first() {
            let existing = self
                .get_item_detail(&item_id)?
                .and_then(|item| item.readable);
            if body.blob.blob_id == blob_id && existing.is_none() {
                let pending = self
                    .query_with_params(
                        "?[blob_id] := *social_blob_pending{share_id:$id,blob_id}",
                        BTreeMap::from([("id".into(), DataValue::from(share_id))]),
                    )?
                    .rows
                    .into_iter()
                    .map(|row| string(&row[0]))
                    .collect::<Result<std::collections::BTreeSet<_>>>()?;
                let scope: Vec<_> = share
                    .article
                    .resources
                    .iter()
                    .filter(|image| !pending.contains(&image.blob_id))
                    .cloned()
                    .collect();
                params.extend([
                    ("size".into(), DataValue::from(reference.byte_size as i64)),
                    ("extractor".into(), optional(body.extracted_with.as_deref())),
                    ("images".into(), image_value(&scope)?),
                ]);
                script.push_str("{ ?[item_id,format,blob_id,fetched_at,extracted_with,byte_size] := *item{id:$item,discovered_at:fetched_at}, item_id=$item,format='html_readable',blob_id=$blob,extracted_with=$extractor,byte_size=$size :put item_content {item_id,format => blob_id,fetched_at,extracted_with,byte_size} } { ?[item_id,blob_id,images] <- [[$item,$blob,$images]] :put item_current_image_scope {item_id,blob_id => images} }");
            } else if blob_id != body.blob.blob_id && share.article.resources.contains(&reference) {
                // A source transfer, not the presence of an unrelated cached
                // digest, grants this image to this exact readable body.
                let known_scope = self.current_image_scope(&item_id, &body.blob.blob_id)?;
                let can_scope = existing.as_ref().is_none_or(|known| {
                    known.blob_id == body.blob.blob_id && known_scope.is_some()
                });
                let mut scope = known_scope.unwrap_or_default();
                if can_scope {
                    if !scope.contains(&reference) {
                        scope.push(reference);
                        scope.sort_by(|a, b| a.blob_id.cmp(&b.blob_id));
                    }
                    params.insert("body".into(), DataValue::from(body.blob.blob_id.as_str()));
                    params.insert("images".into(), image_value(&scope)?);
                    script.push_str("{ ?[item_id,blob_id,images] <- [[$item,$body,$images]] :put item_current_image_scope {item_id,blob_id => images} }");
                }
            }
        }
        script.push_str(" { ?[id,token] := id=$item,token=to_string(rand_uuid_v4()) :put self_item_dirty {id => token} } { ?[share_id,blob_id] <- [[$id,$blob]] :rm social_blob_pending {share_id,blob_id} }");
        self.query_with_params(&script, params)?;
        self.social_revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    /// Approved current images for exactly this body. None means a legacy own archive.
    pub fn current_image_scope(
        &self,
        item_id: &str,
        blob_id: &str,
    ) -> Result<Option<Vec<BlobReference>>> {
        let rows = self.query_with_params(
            "?[images] := *item_current_image_scope{item_id:$id,blob_id:$blob,images}",
            BTreeMap::from([
                ("id".into(), DataValue::from(item_id)),
                ("blob".into(), DataValue::from(blob_id)),
            ]),
        )?;
        rows.rows
            .first()
            .map(|row| match &row[0] {
                DataValue::Json(value) => serde_json::from_value(value.0.clone())
                    .map_err(|_| invalid("invalid approved image scope")),
                _ => Err(invalid("invalid approved image scope")),
            })
            .transpose()
    }
    /// Original active received vouches for an article, newest first.
    pub fn received_item_shares(&self, item_id: &str, limit: usize) -> Result<Vec<ReceivedShare>> {
        let rows=self.query_with_params("retracted[id,author,recipient] := *social_record{share_id:id,author,recipient,kind:'retraction'} ?[key,name,created_at] := *social_receipt{share_id,item_id:$id}, *social_record{key,share_id,kind:'share',author,recipient,created_at}, not retracted[share_id,author,recipient], *social_contact{master:author,name} :sort -created_at, key :limit $limit",BTreeMap::from([("id".into(),DataValue::from(item_id)),("limit".into(),DataValue::from(limit.clamp(1,100) as i64))]))?;
        rows.rows
            .iter()
            .map(|row| {
                let Some(ShareFact::Share(share)) = self.social_record(&string(&row[0])?)? else {
                    return Err(invalid("invalid received share"));
                };
                Ok(ReceivedShare {
                    share: *share,
                    item_id: item_id.into(),
                    name: string(&row[1])?,
                })
            })
            .collect()
    }
    /// Local namespace binding kept apart from the self document.
    pub fn social_document(&self) -> Result<Option<SocialDocument>> {
        let rows = self.query(
            "?[namespace,capability] := *social_document{singleton:0,namespace,capability}",
        )?;
        rows.rows
            .first()
            .map(|row| {
                let DataValue::Bytes(namespace) = &row[0] else {
                    return Err(invalid("invalid social namespace"));
                };
                let DataValue::Bytes(capability) = &row[1] else {
                    return Err(invalid("invalid social capability"));
                };
                Ok(SocialDocument {
                    namespace: namespace
                        .as_slice()
                        .try_into()
                        .map_err(|_| invalid("invalid social namespace length"))?,
                    capability: capability.clone(),
                })
            })
            .transpose()
    }

    /// Own recent vouches for deliberate review and withdrawal in People settings.
    pub fn sent_social_shares(&self, limit: usize) -> Result<Vec<SentShare>> {
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        let rows=self.query_with_params("?[key,created_at] := *social_record{key,kind:'share',author:$author,created_at} :sort -created_at,key :limit $limit",BTreeMap::from([("author".into(),key_value(binding.master_pubkey)),("limit".into(),DataValue::from(limit.clamp(1,100) as i64))]))?;
        rows.rows
            .iter()
            .map(|row| {
                let Some(ShareFact::Share(share)) = self.social_record(&string(&row[0])?)? else {
                    return Err(invalid("missing sent share"));
                };
                let name = self
                    .social_contact(&share.recipient)?
                    .map(|contact| contact.name)
                    .unwrap_or_else(|| share.recipient.to_hex()[..12].into());
                let retracted = self.social_share_retracted(
                    &share.id,
                    &share.certificate.master_pubkey,
                    &share.recipient,
                )?;
                Ok(SentShare {
                    share: *share,
                    name,
                    retracted,
                })
            })
            .collect()
    }
    /// Bind once; cache loss can reopen the same namespace without changing identity.
    pub fn bind_social_document(&self, document: &SocialDocument) -> Result<()> {
        if document.capability.is_empty() || document.capability.len() > 64 * 1024 {
            return Err(invalid("invalid social document capability"));
        }
        self.query_with_params("?[singleton,namespace,capability] <- [[0,$namespace,$capability]] :put social_document {singleton => namespace,capability}",BTreeMap::from([("namespace".into(),DataValue::Bytes(document.namespace.to_vec())),("capability".into(),DataValue::Bytes(document.capability.clone()))]))?;
        Ok(())
    }
}

fn record_params(fact: &ShareFact) -> Result<BTreeMap<String, DataValue>> {
    Ok(BTreeMap::from([
        ("key".into(), DataValue::from(fact.key()?)),
        ("encoded".into(), DataValue::Bytes(fact.to_bytes()?)),
        ("author".into(), key_value(fact.author())),
        ("recipient".into(), key_value(fact.recipient())),
        ("share_id".into(), DataValue::from(fact.share_id())),
        ("kind".into(), DataValue::from(fact.kind())),
        ("created".into(), DataValue::from(fact.created_at_micros())),
    ]))
}
fn image_value(images: &[BlobReference]) -> Result<DataValue> {
    Ok(DataValue::Json(JsonData(
        serde_json::to_value(images).map_err(|_| invalid("invalid image scope"))?,
    )))
}
fn key_value(key: PublicKey) -> DataValue {
    DataValue::Bytes(key.as_bytes().to_vec())
}
fn public_key(value: &DataValue) -> Result<PublicKey> {
    match value {
        DataValue::Bytes(bytes) => {
            PublicKey::from_slice(bytes).map_err(|_| invalid("invalid stored public key"))
        }
        _ => Err(invalid("invalid stored public key")),
    }
}
fn string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        _ => Err(invalid("invalid stored string")),
    }
}
fn boolean(value: &DataValue) -> Result<bool> {
    match value {
        DataValue::Bool(value) => Ok(*value),
        _ => Err(invalid("invalid stored boolean")),
    }
}
fn validity(time: i64) -> DataValue {
    DataValue::Validity(Validity::from((time, true)))
}
fn optional(value: Option<&str>) -> DataValue {
    value.map(DataValue::from).unwrap_or(DataValue::Null)
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "private sharing",
        message: message.into(),
    }
}
