//! Contextual introductions reuse explicit, key-pinned contact-code review.
use super::{
    conversations::{Participant, participant},
    sharing::{ContactPreview, parse_contact, parse_key, wake},
};
use crate::{AppError, Result, state::AppState};
use carrel_core::{identity::PublicKey, sharing::ContactCard};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationPerson {
    person: Participant,
    introduced_by: String,
    is_self: bool,
    followed: bool,
    connected: bool,
}

#[tauri::command]
pub(crate) async fn get_conversation_person(
    state: State<'_, AppState>,
    share_id: String,
    key: String,
) -> Result<ConversationPerson> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = parse_key(&key)?;
        let root = store.conversation_participant(&share_id, &key)?;
        let local = store
            .self_identity()?
            .ok_or_else(|| AppError::Config("Identity is unavailable.".into()))?;
        let contact = store.social_contact(&key)?;
        Ok(ConversationPerson {
            person: participant(&store, key)?,
            introduced_by: store.social_name(&root.certificate.master_pubkey)?,
            is_self: key == local.master_pubkey,
            followed: contact.as_ref().is_some_and(|contact| contact.followed),
            connected: contact
                .as_ref()
                .is_some_and(|contact| contact.followed && contact.card.is_some()),
        })
    })
    .await
    .map_err(|_| AppError::Config("Could not open this person. Try again.".into()))?
}

fn reviewed_code(
    store: &carrel_store::Store,
    share_id: &str,
    key: PublicKey,
    contact: &str,
) -> Result<ContactCard> {
    store.conversation_participant(share_id, &key)?;
    let (actual, card) = parse_contact(contact)?;
    if actual != key {
        return Err(AppError::Config("This code belongs to a different person. Ask this participant for their contact code; your draft is still here.".into()));
    }
    card.ok_or_else(|| {
        AppError::Config("Paste this person's signed contact code to connect this desktop.".into())
    })
}

#[tauri::command]
pub(crate) fn inspect_conversation_person(
    state: State<'_, AppState>,
    share_id: String,
    key: String,
    contact: String,
) -> Result<ContactPreview> {
    let key = parse_key(&key)?;
    reviewed_code(&state.store, &share_id, key, &contact)?;
    Ok(ContactPreview {
        key: key.to_hex(),
        connection_info: true,
    })
}

#[tauri::command]
pub(crate) async fn connect_conversation_person(
    state: State<'_, AppState>,
    share_id: String,
    key: String,
    contact: String,
    name: String,
) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = parse_key(&key)?;
        let card = reviewed_code(&context.store, &share_id, key, &contact)?;
        let device = super::highlights::load_device_key(&context.paths)?;
        context.store.social_local_certificate(device.public())?;
        context
            .store
            .follow_social_contact(key, &name, Some(&card))?;
        context
            .store
            .publish_social_profile_to(key, crate::feed_runner::now_micros(), &device)?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|_| {
        AppError::Config("Could not save this connection. Your code is still here.".into())
    })??;
    wake(&state).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::{
        identity::{DeviceCert, Keypair},
        private_library::SavedItem,
        sharing::{Share, ShareFact},
    };

    #[test]
    fn introduction_review_pins_the_participant_without_following_or_disclosing_contacts() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let local = Keypair::from_seed(&[161; 32]);
        let local_device = Keypair::from_seed(&[162; 32]);
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&local, local_device.public(), 1, None),
                &local_device,
            )
            .unwrap();
        let sharer = Keypair::from_seed(&[163; 32]);
        let sharer_device = Keypair::from_seed(&[164; 32]);
        let friend = Keypair::from_seed(&[165; 32]);
        let friend_device = Keypair::from_seed(&[166; 32]);
        let outsider = Keypair::from_seed(&[167; 32]);
        store
            .follow_social_contact(*sharer.public(), "Chris", None)
            .unwrap();
        let share = Share::sign_for(
            "introduction".into(),
            vec![*local.public(), *friend.public()],
            SavedItem {
                id: "a".repeat(64),
                kind: "article".into(),
                title: "A shared context".into(),
                creators: vec![],
                primary_url: Some("https://example.org/context".into()),
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
            None,
            1,
            DeviceCert::new_at_micros(&sharer, sharer_device.public(), 1, None),
            &sharer_device,
        )
        .unwrap();
        store
            .accept_social_record(
                &ShareFact::Share(Box::new(share.clone())),
                sharer.public(),
                2,
                &local_device,
            )
            .unwrap();
        let code = ContactCard::sign(
            DeviceCert::new_at_micros(&friend, friend_device.public(), 1, None),
            serde_json::to_string(&iroh::EndpointAddr::new(
                iroh::PublicKey::from_bytes(friend_device.public().as_bytes()).unwrap(),
            ))
            .unwrap(),
            &friend_device,
        )
        .unwrap();
        assert!(
            store
                .conversation_participant(&share.id, friend.public())
                .is_ok()
        );
        assert!(
            store
                .conversation_participant(&share.id, outsider.public())
                .is_err()
        );
        assert_eq!(
            reviewed_code(
                &store,
                &share.id,
                *friend.public(),
                &code.to_text().unwrap()
            )
            .unwrap(),
            code
        );
        assert!(
            reviewed_code(
                &store,
                &share.id,
                *sharer.public(),
                &code.to_text().unwrap()
            )
            .is_err()
        );
        assert!(
            reviewed_code(
                &store,
                &share.id,
                *outsider.public(),
                &code.to_text().unwrap()
            )
            .is_err()
        );
        assert!(
            reviewed_code(
                &store,
                "missing",
                *friend.public(),
                &code.to_text().unwrap()
            )
            .is_err()
        );
        assert!(
            reviewed_code(
                &store,
                &share.id,
                *friend.public(),
                &friend.public().to_hex()
            )
            .is_err()
        );
        let mut forged = code;
        forged.address = "changed address".into();
        assert!(
            reviewed_code(
                &store,
                &share.id,
                *friend.public(),
                &format!("carrel-contact:{}", serde_json::to_string(&forged).unwrap())
            )
            .is_err()
        );
        assert!(
            store.social_contact(friend.public()).unwrap().is_none(),
            "opening and reviewing a card never follows a person"
        );
        assert!(store.social_contact(outsider.public()).unwrap().is_none());
    }
}
