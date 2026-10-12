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
    key: Option<String>,
    contact_code: Option<String>,
    ready: bool,
    people: Vec<PersonSummary>,
    shares: Vec<SentShareSummary>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersonSummary {
    key: String,
    name: String,
    connected: bool,
    followed: bool,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SentShareSummary {
    id: String,
    item_id: String,
    title: String,
    to: String,
    note: Option<String>,
    date: String,
    retracted: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareRequest {
    request_id: String,
    item_id: String,
    recipient: String,
    note: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContactPreview {
    key: String,
    connection_info: bool,
}

#[tauri::command]
pub(crate) async fn list_people(state: State<'_, AppState>) -> Result<PeopleSnapshot> {
    let binding = state.store.self_identity()?;
    let sync = state.sync.lock().await;
    let service = sync.social.as_ref();
    let people = state
        .store
        .social_contacts()?
        .into_iter()
        .map(|contact| PersonSummary {
            key: contact.master.to_hex(),
            name: contact.name,
            connected: contact.card.is_some(),
            followed: contact.followed,
            error: service.and_then(|worker| worker.error(&contact.master)),
        })
        .collect();
    let shares = if binding.is_some() {
        state
            .store
            .sent_social_shares(100)?
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
            .collect()
    } else {
        Vec::new()
    };
    let contact_code = service
        .and_then(|worker| worker.bridge.contact_card().ok())
        .and_then(|card| card.to_text().ok());
    Ok(PeopleSnapshot {
        key: binding.map(|binding| binding.master_pubkey.to_hex()),
        ready: contact_code.is_some(),
        contact_code,
        people,
        shares,
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
    let to = state
        .store
        .social_contact(&share.recipient)?
        .map(|person| person.name)
        .unwrap_or_else(|| share.recipient.to_hex()[..12].into());
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
    let snapshot = store
        .saved_article_snapshot(&request.item_id)?
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
    let article = public_article_snapshot(snapshot, images)
        .map_err(|_| AppError::Config("This article cannot be prepared for sharing.".into()))?;
    let note = (!request.note.trim().is_empty()).then(|| request.note.trim().to_owned());
    Ok(store.create_social_share(
        ShareDraft {
            request_id: request.request_id,
            recipient: parse_key(&request.recipient)?,
            article,
            note,
        },
        crate::feed_runner::now_micros(),
        device,
    )?)
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
async fn wake(state: &AppState) {
    let sync = state.sync.lock().await;
    if let Some(worker) = &sync.social {
        worker.wake();
    }
}
async fn refresh_authorization(state: &AppState) -> Result<()> {
    crate::sync::refresh_sharing_authorization(state).await
}
fn parse_key(key: &str) -> Result<PublicKey> {
    let key = PublicKey::from_hex(key.trim())
        .map_err(|_| AppError::Config("Enter a valid public key or Carrel contact code.".into()))?;
    key.validate()
        .map_err(|_| AppError::Config("This public key is not valid.".into()))?;
    Ok(key)
}
fn parse_contact(text: &str) -> Result<(PublicKey, Option<ContactCard>)> {
    if text.trim().starts_with("carrel-contact:") {
        let card = ContactCard::from_text(text)
            .map_err(|_| AppError::Config("This contact code could not be verified.".into()))?;
        carrel_sync::sharing::contact_address(&card)?;
        Ok((card.certificate.master_pubkey, Some(card)))
    } else {
        Ok((parse_key(text)?, None))
    }
}
fn date(at: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1000)
        .map(|at| at.date().to_string())
        .unwrap_or_else(|_| "unknown date".into())
}
