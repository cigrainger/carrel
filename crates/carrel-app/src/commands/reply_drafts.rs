//! Native composition commands; unsent text never wakes or enters social sync.
use crate::{AppError, Result, state::AppState};
use carrel_core::conversation::{ReplyDraft, ReplyDraftInput, ReplyDraftSnapshot};
use serde::{Deserialize, Serialize};
use tauri::State;

/// Revisions cross the webview boundary as strings, including above 2^53.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftSnapshot {
    revision: String,
    draft: Option<SavedDraft>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedDraft {
    body: String,
    parent: Option<String>,
    request_id: String,
    retry_device: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftInput {
    body: String,
    parent: Option<String>,
    request_id: String,
}

impl From<ReplyDraftSnapshot> for DraftSnapshot {
    fn from(snapshot: ReplyDraftSnapshot) -> Self {
        Self {
            revision: snapshot.revision.to_string(),
            draft: snapshot.draft.map(|draft| SavedDraft {
                body: draft.body,
                parent: draft.parent,
                request_id: draft.request_id,
                retry_device: draft.retry_device.to_hex(),
            }),
        }
    }
}

impl DraftSnapshot {
    fn decode(self) -> Result<ReplyDraftSnapshot> {
        let snapshot = ReplyDraftSnapshot {
            revision: revision(&self.revision)?,
            draft: self
                .draft
                .map(|draft| {
                    Ok::<_, AppError>(ReplyDraft {
                        body: draft.body,
                        parent: draft.parent,
                        request_id: draft.request_id,
                        retry_device: super::sharing::parse_key(&draft.retry_device)?,
                    })
                })
                .transpose()?,
        };
        snapshot.verify().map_err(|_| {
            AppError::Config("Could not read this draft. Your text is still here.".into())
        })?;
        Ok(snapshot)
    }
}

fn revision(text: &str) -> Result<i64> {
    text.parse::<i64>()
        .ok()
        .filter(|value| *value >= 0 && value.to_string() == text)
        .ok_or_else(|| {
            AppError::Config(
                "Could not read this draft's saved revision. Reload it to retry.".into(),
            )
        })
}

#[tauri::command]
pub(crate) async fn load_reply_draft(
    state: State<'_, AppState>,
    share_id: String,
) -> Result<DraftSnapshot> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, AppError>(store.load_reply_draft(&share_id)?.into())
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "load reply draft",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn save_reply_draft(
    state: State<'_, AppState>,
    share_id: String,
    expected_revision: String,
    draft: DraftInput,
) -> Result<DraftSnapshot> {
    let expected = revision(&expected_revision)?;
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let device = super::highlights::load_device_key(&context.paths)?;
        Ok::<_, AppError>(
            context
                .store
                .save_reply_draft(
                    &share_id,
                    expected,
                    ReplyDraftInput {
                        body: draft.body,
                        parent: draft.parent,
                        request_id: draft.request_id,
                    },
                    device.public(),
                )?
                .into(),
        )
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "save reply draft",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn discard_reply_draft(
    state: State<'_, AppState>,
    share_id: String,
    expected_revision: String,
) -> Result<DraftSnapshot> {
    let expected = revision(&expected_revision)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, AppError>(store.discard_reply_draft(&share_id, expected)?.into())
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "discard reply draft",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn send_reply_draft(
    state: State<'_, AppState>,
    share_id: String,
    snapshot: DraftSnapshot,
) -> Result<DraftSnapshot> {
    let snapshot = snapshot.decode()?;
    let context = state.inner().clone();
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        let device = super::highlights::load_device_key(&context.paths)?;
        Ok::<_, AppError>(
            context
                .store
                .reply_from_draft(
                    &share_id,
                    &snapshot,
                    crate::feed_runner::now_micros(),
                    &device,
                )?
                .draft
                .into(),
        )
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "send reply draft",
        source,
    })??;
    super::sharing::wake(&state).await;
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_use_canonical_lossless_decimal_strings() {
        for value in [0, 1, 9_007_199_254_740_993, i64::MAX] {
            assert_eq!(revision(&value.to_string()).unwrap(), value);
        }
        for value in [
            "",
            "-1",
            "+1",
            "01",
            " 1",
            "1 ",
            "1.0",
            "文",
            "9223372036854775808",
        ] {
            assert!(revision(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn saved_snapshot_round_trip_preserves_exact_text_context_and_retry_identity() {
        let original = ReplyDraftSnapshot {
            revision: 9_007_199_254_740_993,
            draft: Some(ReplyDraft {
                body: " An unfinished thought 文\n\t".into(),
                parent: Some("a".repeat(64)),
                request_id: "local-retry".into(),
                retry_device: *carrel_core::identity::Keypair::from_seed(&[91; 32]).public(),
            }),
        };
        let encoded = serde_json::to_value(DraftSnapshot::from(original.clone())).unwrap();
        assert_eq!(encoded["revision"], "9007199254740993");
        assert!(encoded["draft"]["retryDevice"].is_string());
        let decoded: DraftSnapshot = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.decode().unwrap(), original);
    }

    #[test]
    fn cleared_snapshot_keeps_its_revision_and_empty_input_cannot_choose_a_device() {
        let original = ReplyDraftSnapshot {
            revision: 7,
            draft: None,
        };
        let encoded = serde_json::to_value(DraftSnapshot::from(original.clone())).unwrap();
        assert!(encoded["draft"].is_null());
        let decoded: DraftSnapshot = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.decode().unwrap(), original);
        assert!(
            serde_json::from_value::<DraftInput>(serde_json::json!({
                "body":"", "parent":null, "requestId":"local", "retryDevice":"chosen"
            }))
            .is_err()
        );
    }
}
