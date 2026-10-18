//! Error boundary for Tauri command serialization.

use serde::Serialize;

/// Application result alias.
pub type Result<T> = std::result::Result<T, AppError>;

/// Errors raised by the desktop shell.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Encrypted local backup or staged activation failed.
    #[error(transparent)]
    Backup(#[from] carrel_store::backup::BackupError),
    /// A blocking native operation did not return a result.
    #[error("background operation did not finish: {operation}")]
    BackgroundTask {
        /// Static operation name, without article contents or other private data.
        operation: &'static str,
        /// Runtime failure retained for diagnostics, not serialized into the UI.
        #[source]
        source: tauri::Error,
    },
    /// Explicit article capture failed before it could be archived.
    #[error("{0}")]
    Capture(String),
    /// Explicit desktop pairing or private synchronization failed.
    #[error(transparent)]
    Sync(#[from] carrel_sync::SyncError),
    /// Local configuration could not be resolved.
    #[error("{0}")]
    Config(String),

    /// The store failed to open, migrate, or answer a query.
    #[error(transparent)]
    Store(#[from] carrel_store::StoreError),

    /// The blob store failed to read cached content.
    #[error(transparent)]
    Blob(#[from] carrel_store::blobs::BlobError),

    /// The local device key could not be loaded.
    #[error(transparent)]
    Keystore(#[from] carrel_store::keystore::KeystoreError),

    /// Feed subscription or fetch orchestration failed.
    #[error("{0}")]
    Feed(String),

    /// Subscription interchange could not read, validate, or save a file.
    #[error("{0}")]
    SubscriptionFile(String),

    /// Annotation export could not save a file.
    #[error("{0}")]
    AnnotationFile(String),

    /// Tauri failed while building or running the shell.
    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    /// The operating system could not open an article link.
    #[error(transparent)]
    OpenLink(#[from] tauri_plugin_opener::Error),

    /// The native app-link integration failed to initialize.
    #[error(transparent)]
    AppLink(#[from] tauri_plugin_deep_link::Error),

    /// A Tauri command received or produced malformed data.
    #[error("invalid app data for {context}: {value}")]
    InvalidData {
        /// Context where the invalid data appeared.
        context: &'static str,
        /// Debug representation of the invalid value.
        value: String,
    },
}

#[derive(Serialize)]
struct SerializedAppError {
    kind: &'static str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let kind = match self {
            Self::Backup(_) => "backup",
            Self::BackgroundTask { .. } => "background_task",
            Self::Capture(_) => "capture",
            Self::Sync(carrel_sync::SyncError::InvalidInvitation) => "pairing_invitation",
            Self::Sync(_) => "sync",
            Self::Config(_) => "config",
            Self::Store(carrel_store::StoreError::AnnotationConflict { .. }) => {
                "annotation_conflict"
            }
            Self::Store(carrel_store::StoreError::ReplyDraftConflict { .. }) => "draft_conflict",
            Self::Store(carrel_store::StoreError::ReplyDraftRevisionExhausted { .. }) => {
                "draft_revision_exhausted"
            }
            Self::Store(carrel_store::StoreError::ConversationUnavailable(participation)) => {
                use carrel_core::conversation::ConversationParticipation;
                match participation {
                    ConversationParticipation::Pending => "conversation_pending",
                    ConversationParticipation::Declined => "conversation_declined",
                    ConversationParticipation::Left => "conversation_left",
                    ConversationParticipation::Admitted => "conversation_unavailable",
                }
            }
            Self::Store(_) => "store",
            Self::Blob(_) => "blob",
            Self::Keystore(carrel_store::keystore::KeystoreError::WrongPassphrase) => {
                "recovery_passphrase"
            }
            Self::Keystore(_) => "keystore",
            Self::Feed(_) => "feed",
            Self::SubscriptionFile(_) => "subscription_file",
            Self::AnnotationFile(_) => "annotation_file",
            Self::Tauri(_) => "tauri",
            Self::OpenLink(_) => "open_link",
            Self::AppLink(_) => "app_link",
            Self::InvalidData { .. } => "invalid_data",
        };

        let message = match self {
            Self::Store(carrel_store::StoreError::ReplyDraftConflict { .. }) =>
                "The saved draft changed. Your text is still here. Reload the saved draft and review it before trying again.".into(),
            Self::Store(carrel_store::StoreError::ReplyDraftRevisionExhausted { .. }) =>
                "Could not save this draft. Your text is still here; copy it before closing Carrel.".into(),
            Self::Store(carrel_store::StoreError::ConversationUnavailable(participation)) => {
                use carrel_core::conversation::ConversationParticipation;
                match participation {
                    ConversationParticipation::Pending => "This conversation is still syncing. Your draft is still here.",
                    ConversationParticipation::Declined => "This group conversation was declined under your conversation preference. Your draft is still here.",
                    ConversationParticipation::Left => "You left this conversation. Your draft is still here.",
                    ConversationParticipation::Admitted => "This conversation is unavailable. Refresh it to check its current state. Your draft is still here.",
                }.to_owned()
            }
            _ => self.to_string(),
        };
        SerializedAppError { kind, message }.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_errors_keep_their_recovery_kind_without_serializing_store_metadata() {
        for (source, kind) in [
            (
                carrel_store::StoreError::ReplyDraftConflict {
                    share_id: "private-root".into(),
                    expected_revision: 9,
                    current_revision: 10,
                },
                "draft_conflict",
            ),
            (
                carrel_store::StoreError::ReplyDraftRevisionExhausted {
                    share_id: "private-root".into(),
                },
                "draft_revision_exhausted",
            ),
        ] {
            let value = serde_json::to_value(AppError::Store(source)).unwrap();
            assert_eq!(value["kind"], kind);
            assert!(
                value["message"]
                    .as_str()
                    .unwrap()
                    .contains("text is still here")
            );
            assert!(!value.to_string().contains("private-root"));
        }
    }

    #[test]
    fn conversation_refusals_keep_typed_reasons_at_the_ui_boundary() {
        use carrel_core::conversation::ConversationParticipation;
        for (participation, kind) in [
            (ConversationParticipation::Pending, "conversation_pending"),
            (ConversationParticipation::Declined, "conversation_declined"),
            (ConversationParticipation::Left, "conversation_left"),
            (
                ConversationParticipation::Admitted,
                "conversation_unavailable",
            ),
        ] {
            let error = AppError::Store(carrel_store::StoreError::ConversationUnavailable(
                participation,
            ));
            let encoded = serde_json::to_value(error).unwrap();
            assert_eq!(encoded["kind"], kind);
            assert!(
                encoded["message"]
                    .as_str()
                    .unwrap()
                    .contains("draft is still here")
            );
        }
    }

    #[test]
    fn pairing_recovery_errors_have_typed_ui_kinds() {
        for (error, kind) in [
            (
                AppError::Sync(carrel_sync::SyncError::InvalidInvitation),
                "pairing_invitation",
            ),
            (
                AppError::Keystore(carrel_store::keystore::KeystoreError::WrongPassphrase),
                "recovery_passphrase",
            ),
        ] {
            assert_eq!(serde_json::to_value(error).unwrap()["kind"], kind);
        }
    }

    #[tokio::test]
    async fn failed_native_worker_does_not_serialize_its_panic_payload() {
        let source = tauri::async_runtime::spawn_blocking(|| panic!("fixture private content"))
            .await
            .unwrap_err();
        let error = AppError::BackgroundTask {
            operation: "load article",
            source,
        };
        let encoded = serde_json::to_value(error).unwrap();
        assert_eq!(encoded["kind"], "background_task");
        assert_eq!(
            encoded["message"],
            "background operation did not finish: load article"
        );
        assert!(!encoded.to_string().contains("fixture private content"));
    }
}
