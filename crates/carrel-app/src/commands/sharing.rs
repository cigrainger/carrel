//! Native composition of explicit contact, share and withdrawal operations.
use crate::{AppError, Result, state::AppState};
use carrel_core::{
    identity::PublicKey,
    private_library::BlobReference,
    sharing::{ContactCard, public_article_snapshot},
};
use carrel_store::{blobs::BlobId, sharing::ShareDraft};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeopleSnapshot {
    conversation_preferences: Option<ConversationPreferences>,
    profile: Option<super::conversations::ProfileSummary>,
    groups: Vec<super::conversations::GroupSummary>,
    key: Option<String>,
    contact_code: Option<String>,
    ready: bool,
    people: Vec<PersonSummary>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationPreferences {
    allow_unfollowed_group_conversations: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersonSummary {
    image: Option<String>,
    key: String,
    name: String,
    connected: bool,
    followed: bool,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SentShareSummary {
    pub(super) id: String,
    pub(super) item_id: String,
    pub(super) title: String,
    pub(super) to: String,
    pub(super) note: Option<String>,
    pub(super) date: String,
    pub(super) retracted: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareRequest {
    request_id: String,
    item_id: String,
    recipient: String,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    audience: Vec<String>,
    note: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContactPreview {
    pub(super) key: String,
    pub(super) connection_info: bool,
}

#[tauri::command]
pub(crate) async fn list_people(state: State<'_, AppState>) -> Result<PeopleSnapshot> {
    let contacts = state.store.social_contacts()?;
    let sync = state.sync.lock().await;
    let service = sync.social.as_ref();
    let errors = contacts
        .iter()
        .map(|contact| {
            (
                contact.master,
                service.and_then(|worker| worker.error(&contact.master)),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let contact_code = service
        .and_then(|worker| worker.bridge.contact_card().ok())
        .and_then(|card| card.to_text().ok());
    drop(sync);
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let binding = store.self_identity()?;
        let people = contacts
            .into_iter()
            .map(|contact| {
                Ok(PersonSummary {
                    image: super::conversations::profile(&store, &contact.master)?.image,
                    key: contact.master.to_hex(),
                    name: store.social_name(&contact.master)?,
                    connected: contact.card.is_some(),
                    followed: contact.followed,
                    error: errors.get(&contact.master).cloned().flatten(),
                })
            })
            .collect::<Result<_>>()?;
        Ok(PeopleSnapshot {
            conversation_preferences: binding
                .as_ref()
                .map(|_| {
                    store.allow_unfollowed_group_conversations().map(|allow| {
                        ConversationPreferences {
                            allow_unfollowed_group_conversations: allow,
                        }
                    })
                })
                .transpose()?,
            profile: binding
                .as_ref()
                .map(|binding| super::conversations::profile(&store, &binding.master_pubkey))
                .transpose()?,
            groups: super::conversations::groups(&store)?,
            key: binding.map(|binding| binding.master_pubkey.to_hex()),
            ready: contact_code.is_some(),
            contact_code,
            people,
        })
    })
    .await
    .map_err(|_| AppError::Config("Could not read your people. Try again.".into()))?
}

#[tauri::command]
pub(crate) async fn set_conversation_preferences(
    state: State<'_, AppState>,
    allow_unfollowed_group_conversations: bool,
) -> Result<ConversationPreferences> {
    let context = state.inner().clone();
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        let device = super::highlights::load_device_key(&context.paths)?;
        context.store.set_allow_unfollowed_group_conversations(
            allow_unfollowed_group_conversations,
            crate::feed_runner::now_micros(),
            &device,
        )?;
        Ok::<_, AppError>(ConversationPreferences {
            allow_unfollowed_group_conversations: context
                .store
                .allow_unfollowed_group_conversations()?,
        })
    })
    .await
    .map_err(|_| {
        AppError::Config("Could not save your conversation preference. Try again.".into())
    })??;
    wake(&state).await;
    Ok(confirmed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SentSharePage {
    shares: Vec<SentShareSummary>,
    next_cursor: Option<carrel_store::sharing::SentShareCursor>,
}
#[tauri::command]
pub(crate) fn list_sent_shares(
    state: State<'_, AppState>,
    after: Option<carrel_store::sharing::SentShareCursor>,
) -> Result<SentSharePage> {
    if state.store.self_identity()?.is_none() {
        return Ok(SentSharePage {
            shares: Vec::new(),
            next_cursor: None,
        });
    }
    let page = state.store.sent_social_share_page(after.as_ref(), 50)?;
    Ok(SentSharePage {
        shares: page
            .shares
            .into_iter()
            .map(|sent| SentShareSummary {
                id: sent.share.id,
                item_id: sent.share.article.id,
                title: sent.share.article.title,
                to: sent.name,
                note: sent.share.note,
                date: date(sent.share.created_at_micros),
                retracted: sent.retracted,
            })
            .collect(),
        next_cursor: page.next_cursor,
    })
}

#[tauri::command]
pub(crate) fn inspect_person(contact: String) -> Result<ContactPreview> {
    let (key, card) = parse_contact(&contact)?;
    Ok(ContactPreview {
        key: key.to_hex(),
        connection_info: card.is_some(),
    })
}

#[tauri::command]
pub(crate) async fn follow_person(
    state: State<'_, AppState>,
    contact: String,
    name: String,
) -> Result<()> {
    let (key, card) = parse_contact(&contact)?;
    state
        .store
        .follow_social_contact(key, &name, card.as_ref())?;
    let device = super::highlights::load_device_key(&state.paths)?;
    state
        .store
        .publish_social_profile_to(key, crate::feed_runner::now_micros(), &device)?;
    wake(&state).await;
    Ok(())
}
#[tauri::command]
pub(crate) async fn remove_person(state: State<'_, AppState>, key: String) -> Result<()> {
    state.store.unfollow_social_contact(&parse_key(&key)?)?;
    let sync = state.sync.lock().await;
    if let Some(worker) = &sync.social {
        worker.wake();
    }
    // Recheck existing streams before reporting the deliberate removal complete.
    drop(sync);
    refresh_authorization(&state).await
}
#[tauri::command]
pub(crate) async fn refresh_shares(state: State<'_, AppState>) -> Result<()> {
    wake(&state).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn share_article(
    state: State<'_, AppState>,
    request: ShareRequest,
) -> Result<SentShareSummary> {
    let context = state.inner().clone();
    let share = tauri::async_runtime::spawn_blocking(move || prepare_share(&context, request))
        .await
        .map_err(|_| {
            AppError::Config("Could not prepare this share. Your note is still available.".into())
        })??;
    let to = state.store.social_audience_name(&share)?;
    let summary = SentShareSummary {
        id: share.id,
        item_id: share.article.id,
        title: share.article.title,
        to,
        note: share.note,
        date: date(share.created_at_micros),
        retracted: false,
    };
    wake(&state).await;
    Ok(summary)
}
fn prepare_share(state: &AppState, request: ShareRequest) -> Result<carrel_core::sharing::Share> {
    let device = super::highlights::load_device_key(&state.paths)?;
    prepare_share_from_parts(&state.store, &state.blobs, &device, request)
}
fn prepare_share_from_parts(
    store: &carrel_store::Store,
    blobs: &carrel_store::blobs::BlobStore,
    device: &carrel_core::identity::Keypair,
    request: ShareRequest,
) -> Result<carrel_core::sharing::Share> {
    let note = (!request.note.trim().is_empty()).then(|| request.note.trim().to_owned());
    let certificate = store.social_local_certificate(device.public())?;
    let id = carrel_core::sharing::Share::draft_id(&certificate, &request.request_id)
        .map_err(|_| AppError::Config("This share draft is invalid.".into()))?;
    let mut expected = request
        .audience
        .iter()
        .map(|key| parse_key(key))
        .collect::<Result<Vec<_>>>()?;
    expected.sort();
    if let Some(carrel_core::sharing::ShareFact::Share(existing)) =
        store.social_record(&format!("share/{id}"))?
    {
        let same_recipient = if request.group.is_some() {
            existing.version == 2 && existing.recipients() == expected
        } else {
            existing.version == 1 && existing.recipient == parse_key(&request.recipient)?
        };
        if same_recipient && existing.article.id == request.item_id && existing.note == note {
            return Ok(*existing);
        }
        return Err(AppError::Config("This draft was already saved with different recipients or text. Your current draft is still here.".into()));
    }
    let group = request
        .group
        .as_ref()
        .map(|id| {
            store
                .social_groups()?
                .into_iter()
                .find(|group| group.id == *id)
                .ok_or_else(|| {
                    AppError::Config(
                        "This group is no longer available. Choose recipients again.".into(),
                    )
                })
        })
        .transpose()?;
    if let Some(group) = &group
        && group.members != expected
    {
        return Err(AppError::Config("This group changed after you selected it. Choose it again to review the recipients; your note is still here.".into()));
    }
    if group.is_none() && !expected.is_empty() {
        return Err(AppError::Config(
            "Choose a person or group again before sharing.".into(),
        ));
    }
    let article = prepare_article(store, blobs, &request.item_id)?;
    Ok(store.create_social_share_for(
        ShareDraft {
            request_id: request.request_id,
            recipient: if let Some(group) = &group {
                *group
                    .members
                    .first()
                    .ok_or_else(|| AppError::Config("Choose people for this group first.".into()))?
            } else {
                parse_key(&request.recipient)?
            },
            article,
            note,
        },
        group
            .as_ref()
            .map(|group| group.members.clone())
            .unwrap_or_default(),
        crate::feed_runner::now_micros(),
        device,
    )?)
}

pub(super) fn prepare_article(
    store: &carrel_store::Store,
    blobs: &carrel_store::blobs::BlobStore,
    item_id: &str,
) -> Result<carrel_core::private_library::SavedItem> {
    let snapshot = store
        .saved_article_snapshot(item_id)?
        .ok_or_else(|| AppError::Config("This article is no longer available.".into()))?;
    let mut images = Vec::new();
    if let Some(body) = snapshot
        .contents
        .iter()
        .find(|body| body.format == "html_readable")
    {
        let id = body.blob.blob_id.parse::<BlobId>()?;
        let bytes = blobs.get_blocking(&id)?;
        if bytes.len() as u64 != body.blob.byte_size {
            return Err(AppError::Config(
                "The archived text needs recovery before sharing.".into(),
            ));
        }
        let html = std::str::from_utf8(&bytes)
            .map_err(|_| AppError::Config("The archived text could not be read.".into()))?;
        for id in carrel_feeds::cached_image_ids(html) {
            if snapshot
                .current_images
                .as_ref()
                .is_some_and(|scope| !scope.iter().any(|image| image.blob_id == id))
            {
                continue;
            }
            if let Ok(blob) = id.parse::<BlobId>()
                && let Ok(bytes) = blobs.get_blocking(&blob)
            {
                images.push(BlobReference {
                    blob_id: id,
                    byte_size: bytes.len() as u64,
                });
            }
        }
    }
    public_article_snapshot(snapshot, images)
        .map_err(|_| AppError::Config("This article cannot be prepared for sharing.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::identity::{DeviceCert, Keypair};
    use carrel_store::{Store, blobs::BlobStore};
    #[test]
    fn native_share_preparation_and_reader_enforce_exact_current_body_image_scope() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let blobs = BlobStore::open(dir.path());
        let master = Keypair::from_seed(&[61; 32]);
        let device = Keypair::from_seed(&[62; 32]);
        let certificate = DeviceCert::new_at_micros(&master, device.public(), 1, None);
        store.bind_self_identity(&certificate, &device).unwrap();
        let friend = Keypair::from_seed(&[63; 32]);
        store
            .follow_social_contact(*friend.public(), "Friend", None)
            .unwrap();
        let approved = blobs.put_blocking(b"public image").unwrap();
        let private = blobs.put_blocking(b"private cached image").unwrap();
        let html = format!(
            "<p>A public article</p><img src='blob://{approved}'><img src='blob://{private}'><code>blob://{private}</code>"
        );
        let body = blobs.put_blocking(html.as_bytes()).unwrap();
        let url = "https://example.org/scoped";
        let id = carrel_store::ids::id_for_external(url);
        store
            .save_archived_article(
                &carrel_core::article::ArchivedArticle {
                    url: url.into(),
                    aliases: vec![],
                    title: Some("Scoped article".into()),
                    creators: vec![],
                    published_at_micros: Some(42),
                    language: None,
                    summary: None,
                    site_name: None,
                    byline: None,
                    blob_id: body.to_string(),
                    byte_size: html.len() as i64,
                    extracted_with: "test".into(),
                    word_count: 20,
                    estimated_read_minutes: 1,
                    searchable_text: "Public article".into(),
                    shape: Default::default(),
                },
                99,
                None,
            )
            .unwrap();
        let images = vec![BlobReference {
            blob_id: approved.to_string(),
            byte_size: 12,
        }];
        store.query_with_params("?[item_id,blob_id,images] <- [[$id,$body,$images]] :put item_current_image_scope {item_id,blob_id=>images}",std::collections::BTreeMap::from([("id".into(),cozo::DataValue::from(id.as_str())),("body".into(),cozo::DataValue::from(body.to_string())),("images".into(),cozo::DataValue::Json(cozo::JsonData(serde_json::to_value(&images).unwrap())))])) .unwrap();
        let share = prepare_share_from_parts(
            &store,
            &blobs,
            &device,
            ShareRequest {
                request_id: "native-share".into(),
                item_id: id.clone(),
                recipient: friend.public().to_hex(),
                group: None,
                audience: Vec::new(),
                note: "Vouch only".into(),
            },
        )
        .unwrap();
        assert_eq!(share.article.resources, images);
        assert_eq!(share.article.discovered_at_micros, 0);
        assert_eq!(share.note.as_deref(), Some("Vouch only"));
        let detail = super::super::items::get_item_from_parts(&store, &blobs, &id)
            .unwrap()
            .unwrap();
        assert!(
            detail
                .content_html
                .contains(&format!("carrel-blob://{approved}"))
                || detail
                    .content_html
                    .contains(&format!("carrel-blob.localhost/{approved}"))
        );
        assert!(
            !detail
                .content_html
                .contains(&format!("src=\"carrel-blob://{private}"))
        );
        assert!(
            !detail
                .content_html
                .contains(&format!("src=\"http://carrel-blob.localhost/{private}"))
        );
        assert_eq!(detail.read_state, "unread");
        assert!(!detail.starred);
        let another = Keypair::from_seed(&[64; 32]);
        store
            .follow_social_contact(*another.public(), "Another friend", None)
            .unwrap();
        let mut members = vec![*friend.public(), *another.public()];
        members.sort();
        store
            .save_social_group("essays", "Essays", members.clone())
            .unwrap();
        let request = || ShareRequest {
            request_id: "group-retry".into(),
            item_id: id.clone(),
            recipient: String::new(),
            group: Some("essays".into()),
            audience: members.iter().map(|key| key.to_hex()).collect(),
            note: "A deliberately reviewed audience".into(),
        };
        store
            .save_social_group("essays", "Essays", vec![*friend.public()])
            .unwrap();
        assert!(
            prepare_share_from_parts(&store, &blobs, &device, request()).is_err(),
            "a stale selection cannot silently change recipients"
        );
        store
            .save_social_group("essays", "Essays", members.clone())
            .unwrap();
        let group_share = prepare_share_from_parts(&store, &blobs, &device, request()).unwrap();
        let wire = carrel_core::sharing::ShareFact::Share(Box::new(group_share.clone()))
            .to_bytes()
            .unwrap();
        assert!(
            !wire
                .windows(b"Essays".len())
                .any(|bytes| bytes == b"Essays"),
            "local group names never enter the signed wire record"
        );
        store
            .save_social_group("essays", "Renamed", vec![*friend.public()])
            .unwrap();
        assert_eq!(
            prepare_share_from_parts(&store, &blobs, &device, request()).unwrap(),
            group_share,
            "acknowledgement retry preserves the original proof after a group edit"
        );
    }
}

#[tauri::command]
pub(crate) async fn retract_share(state: State<'_, AppState>, id: String) -> Result<()> {
    let device = super::highlights::load_device_key(&state.paths)?;
    state
        .store
        .retract_social_share(&id, crate::feed_runner::now_micros(), &device)?;
    wake(&state).await;
    Ok(())
}
pub(super) async fn wake(state: &AppState) {
    let sync = state.sync.lock().await;
    if let Some(worker) = &sync.social {
        worker.wake();
    }
}
async fn refresh_authorization(state: &AppState) -> Result<()> {
    crate::sync::refresh_sharing_authorization(state).await
}
pub(super) fn parse_key(key: &str) -> Result<PublicKey> {
    let key = PublicKey::from_hex(key.trim())
        .map_err(|_| AppError::Config("Enter a valid public key or Carrel contact code.".into()))?;
    key.validate()
        .map_err(|_| AppError::Config("This public key is not valid.".into()))?;
    Ok(key)
}
pub(super) fn parse_contact(text: &str) -> Result<(PublicKey, Option<ContactCard>)> {
    if text.trim().starts_with("carrel-contact:") {
        let card = ContactCard::from_text(text)
            .map_err(|_| AppError::Config("This contact code could not be verified.".into()))?;
        carrel_sync::sharing::contact_address(&card)?;
        Ok((card.certificate.master_pubkey, Some(card)))
    } else {
        Ok((parse_key(text)?, None))
    }
}
pub(super) fn date(at: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1000)
        .map(|at| at.date().to_string())
        .unwrap_or_else(|_| "unknown date".into())
}
