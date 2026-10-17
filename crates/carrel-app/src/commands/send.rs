//! Audience review and native orchestration of atomic send batches.
use super::{
    conversations::{Participant, participant},
    sharing::{SentShareSummary, date, parse_key, prepare_article, wake},
};
use crate::{AppError, Result, state::AppState};
use carrel_core::sharing::{AudienceSelection, SendDraft, SendMode};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub(crate) struct AudienceReview {
    selection: AudienceSelection,
    recipients: Vec<Participant>,
}

#[tauri::command]
pub(crate) async fn review_share_audience(
    state: State<'_, AppState>,
    people: Vec<String>,
    groups: Vec<String>,
) -> Result<AudienceReview> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let people = people
            .iter()
            .map(|key| parse_key(key))
            .collect::<Result<_>>()?;
        let selection = store.review_social_audience(people, &groups)?;
        let master = store
            .self_identity()?
            .ok_or_else(|| AppError::Config("Create an identity before sharing.".into()))?
            .master_pubkey;
        let recipients = selection
            .recipients(master)
            .map_err(|_| AppError::Config("Choose up to sixteen people.".into()))?
            .into_iter()
            .map(|key| participant(&store, key))
            .collect::<Result<_>>()?;
        Ok(AudienceReview {
            selection,
            recipients,
        })
    })
    .await
    .map_err(|_| AppError::Config("Could not review recipients. Your note is still here.".into()))?
}

#[derive(Serialize)]
pub(crate) struct SendReceipt {
    message: String,
    shares: Vec<SentShareSummary>,
}

#[tauri::command]
pub(crate) async fn send_article_batch(
    state: State<'_, AppState>,
    request: SendDraft,
) -> Result<SendReceipt> {
    let context = state.inner().clone();
    let result=tauri::async_runtime::spawn_blocking(move || {
        let device=super::highlights::load_device_key(&context.paths)?;
        let mode=request.mode;
        let roots=if let Some(roots)=context.store.social_send_retry(request.clone(),device.public())? {roots} else {
            let article=prepare_article(&context.store,&context.blobs,&request.item_id)?;
            context.store.create_social_send(request,article,crate::feed_runner::now_micros(),&device)?
        };
        let names=roots.iter().flat_map(|root|root.recipients()).collect::<std::collections::BTreeSet<_>>().iter().map(|key|context.store.social_name(key)).collect::<carrel_store::Result<Vec<_>>>()?.join(", ");
        let message=match mode {
            SendMode::Separately=>format!("Share saved separately for {names}. Each person has a private conversation with you."),
            SendMode::Conversation=>format!("Conversation saved for {names}. Everyone selected can see the participants and replies."),
        };
        let shares=roots.into_iter().map(|share|Ok(SentShareSummary{
            to:context.store.social_audience_name(&share)?,id:share.id,item_id:share.article.id,title:share.article.title,
            note:share.note,date:date(share.created_at_micros),retracted:false,
        })).collect::<Result<_>>()?;
        Ok::<_,AppError>(SendReceipt{message,shares})
    }).await.map_err(|_|AppError::Config("Could not save this share. Your note and choices are still here.".into()))??;
    wake(&state).await;
    Ok(result)
}
