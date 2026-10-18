//! Atomic local send intent. Separate roots never disclose the batch's audience.
use super::*;
use carrel_core::sharing::{AudienceSelection, SelectedGroup, SendDraft, SendMode};

impl Store {
    /// Expand explicitly selected local groups into the membership being reviewed.
    pub fn review_social_audience(
        &self,
        people: Vec<PublicKey>,
        groups: &[String],
    ) -> Result<AudienceSelection> {
        let _social = self.social_lock.lock().expect("audience review lock");
        if groups.len() > 16 {
            return Err(invalid("choose at most sixteen groups"));
        }
        let known = self.social_groups()?;
        let selection = AudienceSelection {
            people,
            groups: groups
                .iter()
                .map(|id| {
                    let group = known
                        .iter()
                        .find(|group| group.id == *id)
                        .ok_or_else(|| invalid("selected group is unavailable"))?;
                    Ok(SelectedGroup {
                        id: group.id.clone(),
                        members: group.members.clone(),
                    })
                })
                .collect::<Result<_>>()?,
        }
        .normalized()?;
        self.validate_social_selection(&selection)?;
        Ok(selection)
    }

    fn validate_social_selection(&self, selection: &AudienceSelection) -> Result<Vec<PublicKey>> {
        let local = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        let recipients = selection.recipients(local.master_pubkey)?;
        let known = self.social_groups()?;
        for group in &selection.groups {
            if known
                .iter()
                .find(|known| known.id == group.id)
                .is_none_or(|known| known.members != group.members)
            {
                return Err(invalid(
                    "a selected group changed; review its recipients again",
                ));
            }
        }
        for recipient in &recipients {
            self.require_social_contact(recipient)?;
        }
        Ok(recipients)
    }

    /// Recover exact completed proofs before re-reading mutable groups or archive bytes.
    pub fn social_send_retry(
        &self,
        draft: SendDraft,
        device: &PublicKey,
    ) -> Result<Option<Vec<Share>>> {
        let draft = draft.normalized()?;
        let certificate = self.social_local_certificate(device)?;
        self.social_send_retry_unlocked(&draft, &certificate)
    }
    fn social_send_retry_unlocked(
        &self,
        draft: &SendDraft,
        certificate: &DeviceCert,
    ) -> Result<Option<Vec<Share>>> {
        let id = Share::draft_id(certificate, &draft.request_id)?;
        let rows = self.query_with_params(
            "?[request,shares] := *social_send{id:$id,request,shares}",
            BTreeMap::from([("id".into(), DataValue::from(id.as_str()))]),
        )?;
        let Some(row) = rows.rows.first() else {
            return Ok(None);
        };
        let DataValue::Bytes(request) = &row[0] else {
            return Err(invalid("invalid send manifest"));
        };
        if *request != send_request(draft)? {
            return Err(invalid(
                "this send was already saved with different recipients, mode or text",
            ));
        }
        let recipients = draft.selection.recipients(certificate.master_pubkey)?;
        let DataValue::List(keys) = &row[1] else {
            return Err(invalid("invalid send roots"));
        };
        let expected_count = if draft.mode == SendMode::Conversation {
            1
        } else {
            recipients.len()
        };
        if keys.len() != expected_count {
            return Err(invalid("incomplete send manifest"));
        }
        let mut shares = Vec::new();
        for (index, key) in keys.iter().enumerate() {
            let Some(ShareFact::Share(share)) = self.social_record(&string(key)?)? else {
                return Err(invalid("missing sent proof"));
            };
            let expected = if draft.mode == SendMode::Conversation {
                recipients.clone()
            } else {
                vec![recipients[index]]
            };
            let token = send_token(
                &id,
                if draft.mode == SendMode::Separately {
                    Some(recipients[index])
                } else {
                    None
                },
            )?;
            if share.certificate.master_pubkey != certificate.master_pubkey
                || share.certificate.device_pubkey != certificate.device_pubkey
                || share.request_id != token
                || share.article.id != draft.item_id
                || share.note != draft.note
                || share.recipients() != expected
            {
                return Err(invalid("send manifest differs from its signed proofs"));
            }
            shares.push(*share);
        }
        Ok(Some(shares))
    }

    /// Commit all roots and retry metadata together, with no partial audience delivery.
    pub fn create_social_send(
        &self,
        draft: SendDraft,
        article: SavedItem,
        at: i64,
        device: &Keypair,
    ) -> Result<Vec<Share>> {
        let draft = draft.normalized()?;
        let _private = self.private_state_lock.lock().expect("send archive lock");
        let _identity = self.identity_lock.lock().expect("send identity lock");
        let _social = self.social_lock.lock().expect("send write lock");
        let certificate = self.social_local_certificate(device.public())?;
        if let Some(existing) = self.social_send_retry_unlocked(&draft, &certificate)? {
            return Ok(existing);
        }
        let recipients = self.validate_social_selection(&draft.selection)?;
        if article.id != draft.item_id {
            return Err(invalid("send article identity differs"));
        }
        let current = self
            .saved_item_snapshot_unlocked(&article.id)?
            .ok_or_else(|| invalid("article is no longer available"))?;
        if current
            .current_images
            .as_ref()
            .is_some_and(|scope| article.resources.iter().any(|image| !scope.contains(image)))
            || public_article_snapshot(current, article.resources.clone())? != article
        {
            return Err(invalid(
                "article changed while preparing the share; try again",
            ));
        }
        let id = Share::draft_id(&certificate, &draft.request_id)?;
        let shares = match draft.mode {
            SendMode::Conversation => vec![Share::sign_for(
                send_token(&id, None)?,
                recipients,
                article,
                draft.note.clone(),
                at,
                certificate,
                device,
            )?],
            SendMode::Separately => recipients
                .into_iter()
                .map(|recipient| {
                    Ok(Share::sign(
                        send_token(&id, Some(recipient))?,
                        recipient,
                        article.clone(),
                        draft.note.clone(),
                        at,
                        certificate.clone(),
                        device,
                    )?)
                })
                .collect::<Result<_>>()?,
        };
        let mut records = Vec::new();
        let mut private = Vec::new();
        let mut keys = Vec::new();
        for share in &shares {
            let fact = ShareFact::Share(Box::new(share.clone()));
            let params = record_params(&fact)?;
            // Preserve exactly the same journal projection as an individual send.
            records.push(DataValue::List(
                [
                    "key",
                    "encoded",
                    "author",
                    "recipient",
                    "share_id",
                    "kind",
                    "created",
                ]
                .map(|name| params[name].clone())
                .to_vec(),
            ));
            private.push(DataValue::List(vec![
                params["pf_key"].clone(),
                params["pf_encoded"].clone(),
            ]));
            keys.push(DataValue::from(fact.key()?));
        }
        let mut script = String::from(
            r#"
            { ?[key,encoded,author,recipient,share_id,kind,created_at] <- $records :put social_record {key=>encoded,author,recipient,share_id,kind,created_at} }
            { ?[key,encoded] <- $private :put self_fact_history {key=>encoded} }
            { ?[key,encoded] <- $private :put self_fact_outbox {key=>encoded} }
            { ?[id,request,shares] <- [[$id,$request,$shares]] :put social_send {id=>request,shares} }
        "#,
        );
        let mut params = BTreeMap::from([
            ("records".into(), DataValue::List(records)),
            ("private".into(), DataValue::List(private)),
            ("id".into(), DataValue::from(id)),
            ("request".into(), DataValue::Bytes(send_request(&draft)?)),
            ("shares".into(), DataValue::List(keys)),
        ]);
        for (index, share) in shares.iter().enumerate() {
            self.append_social_admission_unlocked(
                &mut script,
                &mut params,
                share,
                at,
                device,
                &format!("batch{index}"),
            )?;
        }
        self.query_with_params(&script, params)?;
        self.social_changed();
        Ok(shares)
    }
}

fn send_request(draft: &SendDraft) -> Result<Vec<u8>> {
    serde_json::to_vec(draft).map_err(|_| invalid("invalid send intent"))
}
fn send_token(id: &str, recipient: Option<PublicKey>) -> Result<String> {
    let bytes = carrel_core::identity::canonical_bytes(&("carrel-send-root-v1", id, recipient))
        .map_err(|_| invalid("invalid send token"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
