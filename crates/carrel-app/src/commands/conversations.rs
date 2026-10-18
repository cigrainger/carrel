//! Native profile, contact-group and fixed-audience conversation composition.
use super::sharing::{date, parse_key, wake};
use crate::{AppError, Result, state::AppState};
use base64::{Engine, engine::general_purpose::STANDARD};
use carrel_core::{
    conversation::{Avatar, ConversationContent as Content, ConversationParticipation},
    sharing::ShareFact,
};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use tauri::{State, Window};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize)]
pub(crate) struct ProfileSummary {
    pub name: String,
    pub avatar: Option<Avatar>,
    pub image: Option<String>,
}
#[derive(Clone, Serialize)]
pub(crate) struct Participant {
    key: String,
    name: String,
    image: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplySummary {
    id: String,
    author: Participant,
    body: Option<String>,
    date: String,
    parent: Option<String>,
    can_remove: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationSummary {
    id: String,
    title: String,
    item_id: String,
    opener: Participant,
    note: Option<String>,
    date: String,
    participants: Vec<Participant>,
    replies: Vec<ReplySummary>,
    closed: bool,
    access: ConversationAccess,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationAccess {
    can_reply: bool,
    can_leave: bool,
    reason: Option<&'static str>,
}

fn conversation_access(
    participation: ConversationParticipation,
    withdrawn: bool,
) -> ConversationAccess {
    let reason = match participation {
        ConversationParticipation::Left => {
            Some("You left this conversation. Its saved article and history remain available.")
        }
        ConversationParticipation::Declined => Some(
            "This group conversation was declined under your conversation preference. You can still read the article and opening note.",
        ),
        _ if withdrawn => {
            Some("This share was withdrawn. Its saved article and history remain available.")
        }
        ConversationParticipation::Pending => Some(
            "This conversation is waiting to finish syncing. You can still read the article and opening note.",
        ),
        ConversationParticipation::Admitted => None,
    };
    let participating = participation == ConversationParticipation::Admitted && !withdrawn;
    ConversationAccess {
        can_reply: participating,
        can_leave: participating,
        reason,
    }
}

// Call only after the store has checked the original root's local reading scope.
// Recovery ownership permits inspection, never an operational sharing action.
fn retained_conversation_access(
    store: &carrel_store::Store,
    id: &str,
    withdrawn: bool,
) -> Result<ConversationAccess> {
    if store.self_identity()?.is_none() {
        return Ok(ConversationAccess {
            can_reply: false,
            can_leave: false,
            reason: Some(
                "This restored conversation is available locally. Resume identity in Settings → Devices before making sharing changes.",
            ),
        });
    }
    Ok(conversation_access(
        store.conversation_participation(id)?,
        withdrawn,
    ))
}
#[derive(Serialize)]
pub(crate) struct Provenance {
    id: String,
    author: String,
    device: String,
    date: String,
    record: String,
    parents: Vec<String>,
}
#[derive(Serialize)]
pub(crate) struct ConversationLink {
    id: String,
    from: String,
    date: String,
    closed: bool,
    access: ConversationAccess,
}

#[tauri::command]
pub(crate) async fn list_article_conversations(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<ConversationLink>> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || article_conversations(&store, &item_id))
        .await
        .map_err(worker_error)?
}

fn article_conversations(
    store: &carrel_store::Store,
    item_id: &str,
) -> Result<Vec<ConversationLink>> {
    store
        .item_conversations(item_id)?
        .into_iter()
        .map(|thread| {
            let access = retained_conversation_access(store, &thread.id, thread.closed)?;
            Ok(ConversationLink {
                id: thread.id,
                from: thread.from,
                date: date(thread.at_micros),
                closed: thread.closed,
                access,
            })
        })
        .collect()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplyRequest {
    share_id: String,
    request_id: String,
    parent: Option<String>,
    body: String,
}
#[derive(Serialize)]
pub(crate) struct GroupSummary {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
}

pub(crate) fn profile(
    store: &carrel_store::Store,
    key: &carrel_core::identity::PublicKey,
) -> Result<ProfileSummary> {
    if let Some(record) = store.social_profile(key)?
        && let Content::Profile { name, avatar, .. } = record.content
    {
        let image = avatar.as_ref().and_then(|a| avatar_image(a).ok());
        return Ok(ProfileSummary {
            name,
            avatar,
            image,
        });
    }
    Ok(ProfileSummary {
        name: String::new(),
        avatar: None,
        image: None,
    })
}
pub(super) fn participant(
    store: &carrel_store::Store,
    key: carrel_core::identity::PublicKey,
) -> Result<Participant> {
    Ok(Participant {
        key: key.to_hex(),
        name: store.social_name(&key)?,
        image: profile(store, &key)?.image,
    })
}
pub(crate) fn groups(store: &carrel_store::Store) -> Result<Vec<GroupSummary>> {
    Ok(store
        .social_groups()?
        .into_iter()
        .map(|g| GroupSummary {
            id: g.id,
            name: g.name,
            members: g.members.into_iter().map(|m| m.to_hex()).collect(),
        })
        .collect())
}

#[tauri::command]
pub(crate) async fn save_profile(
    state: State<'_, AppState>,
    name: String,
    avatar: Option<Avatar>,
) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(avatar) = &avatar {
            avatar_image(avatar)?;
        }
        let device = super::highlights::load_device_key(&context.paths)?;
        context.store.save_social_profile(
            &name,
            avatar,
            crate::feed_runner::now_micros(),
            &device,
        )?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(worker_error)??;
    wake(&state).await;
    Ok(())
}
#[tauri::command]
pub(crate) async fn choose_profile_picture(window: Window) -> Result<Option<ProfileSummary>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Choose a profile picture")
        .add_filter("PNG or JPEG image", &["png", "jpg", "jpeg"])
        .pick_file(move |file| {
            let _ = tx.send(file);
        });
    let Some(file) = rx.await.map_err(worker_error)? else {
        return Ok(None);
    };
    let path = file.into_path().map_err(worker_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(worker_error)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(worker_error)?;
        let avatar = normalize_avatar(&bytes)?;
        let image = Some(avatar_image(&avatar)?);
        Ok(Some(ProfileSummary {
            name: String::new(),
            avatar: Some(avatar),
            image,
        }))
    })
    .await
    .map_err(worker_error)?
}
fn normalize_avatar(bytes: &[u8]) -> Result<Avatar> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(worker_error("picture exceeds 8 MiB"));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(worker_error)?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        return Err(worker_error("choose a PNG or JPEG picture"));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(worker_error)?;
    // Fresh pixel data discards EXIF, comments, profiles and animation metadata.
    let mut side = 256;
    loop {
        let pixels = decoded
            .resize_to_fill(side, side, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(worker_error)?;
        let png = png.into_inner();
        if png.len() <= carrel_core::conversation::MAX_AVATAR_BYTES {
            return Ok(Avatar {
                digest: carrel_store::blobs::BlobId::from_bytes(&png).to_string(),
                png,
            });
        }
        side /= 2;
        if side < 32 {
            return Err(worker_error("picture could not be reduced"));
        }
    }
}
fn avatar_image(avatar: &Avatar) -> Result<String> {
    avatar.verify().map_err(worker_error)?;
    // Never hand an untrusted encoded image directly to the webview. A fresh
    // bounded native decode also makes malicious ancillary PNG data inert.
    let normalized = normalize_avatar(&avatar.png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(normalized.png)
    ))
}
#[tauri::command]
pub(crate) fn save_group(
    state: State<'_, AppState>,
    id: String,
    name: String,
    members: Vec<String>,
) -> Result<()> {
    state.store.save_social_group(
        &id,
        &name,
        members
            .iter()
            .map(|key| parse_key(key))
            .collect::<Result<_>>()?,
    )?;
    Ok(())
}
#[tauri::command]
pub(crate) fn remove_group(state: State<'_, AppState>, id: String) -> Result<()> {
    state.store.remove_social_group(&id)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_conversation(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConversationSummary> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || conversation_summary(&store, &id))
        .await
        .map_err(worker_error)?
}

fn conversation_summary(store: &carrel_store::Store, id: &str) -> Result<ConversationSummary> {
    let conversation = store.social_conversation(id)?;
    let access = retained_conversation_access(store, id, conversation.closed)?;
    let scope = carrel_core::conversation::ThreadScope::from_share(&conversation.share)
        .map_err(worker_error)?;
    let local = store.self_identity()?.map(|binding| binding.master_pubkey);
    let participants = scope
        .participants()
        .into_iter()
        .map(|key| participant(store, key))
        .collect::<Result<Vec<_>>>()?;
    let by_key = participants
        .iter()
        .map(|person| (person.key.clone(), person.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let replies = conversation
        .replies
        .into_iter()
        .map(|reply| {
            let record = reply.record;
            let Content::Reply {
                id, parent, body, ..
            } = record.content
            else {
                return Err(worker_error("invalid reply"));
            };
            let mut author = by_key
                .get(&record.certificate.master_pubkey.to_hex())
                .cloned()
                .ok_or_else(|| worker_error("reply author is outside this audience"))?;
            // Pictures are sent once in the participant list, not once per message.
            author.image = None;
            Ok(ReplySummary {
                id,
                author,
                body: (!reply.removed).then_some(body),
                date: date(record.created_at_micros),
                parent,
                can_remove: !reply.removed && Some(record.certificate.master_pubkey) == local,
            })
        })
        .collect::<Result<_>>()?;
    Ok(ConversationSummary {
        id: id.into(),
        title: conversation.share.article.title,
        item_id: conversation.item_id,
        opener: by_key
            .get(&scope.sharer.to_hex())
            .cloned()
            .ok_or_else(|| worker_error("conversation author is unavailable"))?,
        note: conversation.share.note,
        date: date(conversation.share.created_at_micros),
        participants,
        replies,
        closed: conversation.closed,
        access,
    })
}
#[tauri::command]
pub(crate) async fn leave_conversation(state: State<'_, AppState>, share_id: String) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let device = super::highlights::load_device_key(&context.paths)?;
        context.store.leave_social_conversation(
            &share_id,
            crate::feed_runner::now_micros(),
            &device,
        )?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "leave conversation",
        source,
    })??;
    wake(&state).await;
    Ok(())
}
#[tauri::command]
pub(crate) async fn reply_to_share(
    state: State<'_, AppState>,
    request: ReplyRequest,
) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let device = super::highlights::load_device_key(&context.paths)?;
        context.store.reply_to_social_share(
            &request.share_id,
            &request.request_id,
            request.parent,
            &request.body,
            crate::feed_runner::now_micros(),
            &device,
        )?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(worker_error)??;
    wake(&state).await;
    Ok(())
}
#[tauri::command]
pub(crate) async fn remove_reply(
    state: State<'_, AppState>,
    share_id: String,
    reply_id: String,
) -> Result<()> {
    let device = super::highlights::load_device_key(&state.paths)?;
    state.store.remove_social_reply(
        &share_id,
        &reply_id,
        crate::feed_runner::now_micros(),
        &device,
    )?;
    wake(&state).await;
    Ok(())
}
#[tauri::command]
pub(crate) fn reply_provenance(
    state: State<'_, AppState>,
    share_id: String,
    reply_id: String,
) -> Result<Vec<Provenance>> {
    state
        .store
        .social_reply_ancestry(&share_id, &reply_id)?
        .into_iter()
        .map(|record| {
            let id = if let Content::Reply { id, .. } = &record.content {
                id.clone()
            } else {
                String::new()
            };
            let key = ShareFact::Conversation(Box::new(record.clone()))
                .key()
                .map_err(worker_error)?;
            Ok(Provenance {
                id,
                author: record.certificate.master_pubkey.to_hex(),
                device: record.certificate.device_pubkey.to_hex(),
                date: date(record.created_at_micros),
                record: key,
                parents: if let Content::Reply { parent, .. } = &record.content {
                    parent.iter().cloned().collect()
                } else {
                    Vec::new()
                },
            })
        })
        .collect::<Result<_>>()
}
fn worker_error(error: impl std::fmt::Display) -> AppError {
    AppError::Config(format!("Could not complete this action: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_native_projection_exposes_retained_conversation_without_sharing_actions() {
        use carrel_core::{
            identity::{DeviceCert, Keypair},
            private_library::SavedItem,
            sharing::Share,
        };
        use cozo::DataValue;
        use std::collections::BTreeMap;

        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let master = Keypair::from_seed(&[91; 32]);
        let device = Keypair::from_seed(&[92; 32]);
        let friend = Keypair::from_seed(&[93; 32]);
        let friend_device = Keypair::from_seed(&[94; 32]);
        let recovered = Keypair::from_seed(&[95; 32]);
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&master, device.public(), 1, None),
                &device,
            )
            .unwrap();
        store
            .follow_social_contact(*friend.public(), "Friend", None)
            .unwrap();
        let root = Share::sign(
            "native-recovery".into(),
            *master.public(),
            SavedItem {
                id: "a".repeat(64),
                kind: "article".into(),
                title: "Retained conversation".into(),
                creators: vec![],
                primary_url: Some("https://example.test/native-recovery".into()),
                published_at_micros: None,
                language: None,
                summary: None,
                discovered_at_micros: 0,
                article: None,
                identifiers: vec![],
                contents: vec![],
                resources: vec![],
                current_images: None,
            },
            Some("Opening note".into()),
            2,
            DeviceCert::new_at_micros(&friend, friend_device.public(), 1, None),
            &friend_device,
        )
        .unwrap();
        store
            .accept_social_record(
                &ShareFact::Share(Box::new(root.clone())),
                friend.public(),
                2,
                &device,
            )
            .unwrap();
        let reply = store
            .reply_to_social_share(
                &root.id,
                "local-reply",
                None,
                "Retained reply 文",
                3,
                &device,
            )
            .unwrap();
        let Content::Reply { id: parent, .. } = reply.content else {
            panic!("expected reply");
        };
        let before = conversation_summary(&store, &root.id).unwrap();
        assert!(before.access.can_reply && before.access.can_leave);
        assert!(before.replies[0].can_remove);

        // Model the store's validated restore result at this projection boundary.
        // Encrypted restore/rekeying itself is exercised by the store tests.
        store.query("::set_triggers self_identity").unwrap();
        store
            .query_with_params(
                "{ ?[singleton] <- [[0]] :rm self_identity {singleton} }\n{ ?[singleton,master_pubkey,device_pubkey,legacy_devices] <- [[0,$master,$fresh,[$old]]] :put restore_origin {singleton=>master_pubkey,device_pubkey,legacy_devices} }",
                BTreeMap::from([
                    ("master".into(), DataValue::Bytes(master.public().as_bytes().to_vec())),
                    ("fresh".into(), DataValue::Bytes(recovered.public().as_bytes().to_vec())),
                    ("old".into(), DataValue::Bytes(device.public().as_bytes().to_vec())),
                ]),
            )
            .unwrap();
        let proof_state = store
            .query("?[key,encoded] := *self_fact_history{key,encoded} :sort key")
            .unwrap()
            .rows;
        let restored = conversation_summary(&store, &root.id).unwrap();
        assert!(!restored.access.can_reply && !restored.access.can_leave);
        assert!(restored.access.reason.unwrap().contains("Resume identity"));
        assert!(!restored.replies[0].can_remove);
        assert_eq!(
            restored.replies[0].body.as_deref(),
            Some("Retained reply 文")
        );
        assert_eq!(restored.note, before.note);
        assert_eq!(restored.replies[0].id, parent);
        let links = article_conversations(&store, &restored.item_id).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, root.id);
        assert!(!links[0].access.can_reply && !links[0].access.can_leave);
        assert_eq!(
            store
                .query("?[key,encoded] := *self_fact_history{key,encoded} :sort key")
                .unwrap()
                .rows,
            proof_state,
        );
        assert!(store.conversation_participation(&root.id).is_err());
    }

    #[test]
    fn conversation_access_keeps_consent_separate_from_withdrawal() {
        for participation in [
            ConversationParticipation::Pending,
            ConversationParticipation::Admitted,
            ConversationParticipation::Declined,
            ConversationParticipation::Left,
        ] {
            for withdrawn in [false, true] {
                let access = conversation_access(participation, withdrawn);
                let allowed = participation == ConversationParticipation::Admitted && !withdrawn;
                assert_eq!(access.can_reply, allowed);
                assert_eq!(access.can_leave, allowed);
                assert_eq!(access.reason.is_none(), allowed);
            }
        }
        assert!(
            conversation_access(ConversationParticipation::Left, false)
                .reason
                .unwrap()
                .contains("left")
        );
        assert!(
            conversation_access(ConversationParticipation::Declined, false)
                .reason
                .unwrap()
                .contains("declined")
        );
        assert!(
            conversation_access(ConversationParticipation::Pending, false)
                .reason
                .unwrap()
                .contains("waiting")
        );
        assert!(
            conversation_access(ConversationParticipation::Admitted, true)
                .reason
                .unwrap()
                .contains("withdrawn")
        );
    }

    #[test]
    fn avatar_import_normalizes_bounds_and_discards_original_encoded_metadata() {
        let pixels = image::RgbaImage::from_pixel(1024, 512, image::Rgba([40, 120, 80, 255]));
        let mut original = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut original, image::ImageFormat::Png)
            .unwrap();
        let mut original = original.into_inner();
        original.extend_from_slice(b"private metadata marker");
        let avatar = normalize_avatar(&original).unwrap();
        avatar.verify().unwrap();
        assert!(avatar.png.len() <= carrel_core::conversation::MAX_AVATAR_BYTES);
        assert!(
            !avatar
                .png
                .windows(23)
                .any(|part| part == b"private metadata marker")
        );
        let decoded = image::load_from_memory(&avatar.png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        assert!(
            avatar_image(&avatar)
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(normalize_avatar(&[]).is_err());
        assert!(normalize_avatar(b"<svg>not a raster</svg>").is_err());
        assert!(normalize_avatar(&vec![0; 8 * 1024 * 1024 + 1]).is_err());
        let mut corrupt = avatar;
        corrupt.png.truncate(33);
        corrupt.digest = carrel_store::blobs::BlobId::from_bytes(&corrupt.png).to_string();
        assert!(avatar_image(&corrupt).is_err());
    }

    #[test]
    fn avatar_import_accepts_jpeg_but_rejects_oversized_decoded_dimensions() {
        let pixels = image::RgbImage::from_pixel(32, 64, image::Rgb([40, 120, 80]));
        let mut jpeg = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(pixels)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let avatar = normalize_avatar(&jpeg.into_inner()).unwrap();
        avatar.verify().unwrap();
        let decoded = image::load_from_memory(&avatar.png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));

        // A small encoded file can still exceed the decoder's dimension limit.
        let wide = image::RgbImage::new(4097, 1);
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(wide)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        assert!(normalize_avatar(&png.into_inner()).is_err());
    }
}

#[tauri::command]
pub(crate) async fn note_provenance(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<Vec<Provenance>> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .note_ancestry(&note_id)?
            .into_iter()
            .map(|record| {
                Ok(Provenance {
                    id: record.revision_id.clone(),
                    author: record.created_by.to_hex(),
                    device: record.updated_by.to_hex(),
                    date: date(record.updated_at_micros),
                    record: format!("note/{}", record.revision_id),
                    parents: record.parents,
                })
            })
            .collect::<Result<Vec<_>>>()
    })
    .await
    .map_err(worker_error)?
}
