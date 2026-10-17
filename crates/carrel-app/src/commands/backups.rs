//! Native dialogs and opaque restore reviews; the webview never supplies a path.
use crate::{
    AppError, Result,
    state::{AppState, FeedStatusCounts},
};
use carrel_store::{
    backup::{BackupError, BackupPreview},
    keystore::Keystore,
};
use serde::Serialize;
use tauri::{AppHandle, State, Window};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackupStatus {
    can_restore: bool,
    pending: bool,
    recovery_master: Option<String>,
    recovery_pending: bool,
    has_master_key: bool,
}
#[derive(Serialize)]
pub(crate) struct ReviewPreview {
    #[serde(flatten)]
    preview: BackupPreview,
    date: String,
}

#[derive(Serialize)]
pub(crate) struct RecoveredDesktopPreview {
    master: String,
    key: String,
    name: String,
}

fn recovered_card(
    store: &carrel_store::Store,
    code: &str,
) -> Result<carrel_core::sharing::ContactCard> {
    let card = carrel_core::sharing::ContactCard::from_text(code).map_err(|_| {
        AppError::Config("Paste the complete contact code from your recovered desktop.".into())
    })?;
    carrel_sync::sharing::contact_address(&card)?;
    let binding = store
        .self_identity()?
        .ok_or(carrel_store::StoreError::IdentityUnbound)?;
    if card.certificate.master_pubkey != binding.master_pubkey {
        return Err(AppError::Config("This code belongs to another reading identity. Your device authorizations are unchanged.".into()));
    }
    if card.certificate.device_pubkey == binding.master_pubkey {
        return Err(AppError::Config(
            "A master recovery key cannot be used as a desktop's operational key.".into(),
        ));
    }
    if card.certificate.device_pubkey == binding.device_pubkey {
        return Err(AppError::Config(
            "This is the current desktop's code. Copy the code on your recovered desktop instead."
                .into(),
        ));
    }
    if let Some(existing) = store
        .self_devices()?
        .into_iter()
        .find(|d| d.certificate.device_pubkey == card.certificate.device_pubkey)
        && (!existing.is_authorized() || existing.certificate != card.certificate)
    {
        return Err(AppError::Config("This device is revoked, blocked or has a different certificate. Its authorization cannot be replaced.".into()));
    }
    Ok(card)
}

#[tauri::command]
pub(crate) async fn inspect_recovered_desktop(
    state: State<'_, AppState>,
    code: String,
) -> Result<RecoveredDesktopPreview> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let card = recovered_card(&store, &code)?;
        Ok(RecoveredDesktopPreview {
            master: card.certificate.master_pubkey.to_hex(),
            key: card.certificate.device_pubkey.to_hex(),
            name: card
                .certificate
                .device_name
                .unwrap_or_else(|| "Recovered desktop".into()),
        })
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "review recovered desktop",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn connect_recovered_desktop(
    state: State<'_, AppState>,
    code: String,
) -> Result<()> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let card = recovered_card(&store, &code)?;
        store.register_recovered_desktop(&card)?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "authorize recovered desktop",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn backup_status(state: State<'_, AppState>) -> Result<BackupStatus> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let origin = context.store.restore_origin()?;
        let recovery_pending = origin.is_some() && context.store.self_identity()?.is_none();
        Ok(BackupStatus {
            can_restore: crate::backup::eligible(&context.store, &context.paths)?,
            pending: crate::backup::pending(&context.paths),
            recovery_master: origin.and_then(|o| o.master_pubkey.map(|k| k.to_hex())),
            recovery_pending,
            has_master_key: Keystore::open(&context.paths.keys)
                .encrypted_master()?
                .is_some(),
        })
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "check backup status",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn create_backup(
    window: Window,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<bool> {
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.chars().count() < 12 || passphrase.len() > 4096 {
        return Err(BackupError::Passphrase.into());
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Create encrypted backup")
        .set_file_name("Carrel.carrel-backup")
        .add_filter("Encrypted Carrel backup", &["carrel-backup"])
        .save_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver
        .await
        .map_err(|_| AppError::Config("Backup file selection closed.".into()))?
    else {
        return Ok(false);
    };
    let path = file.into_path().map_err(|_| BackupError::Format)?;
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let parent = path.parent().ok_or(BackupError::Format)?;
        if parent
            .canonicalize()
            .map_err(BackupError::Io)?
            .starts_with(context.paths.root.canonicalize().map_err(BackupError::Io)?)
        {
            return Err(AppError::Config(
                "Choose a backup destination outside Carrel's working data folder.".into(),
            ));
        }
        let _guard = context
            .annotation_lock
            .lock()
            .expect("backup key setup lock");
        crate::backup::write_backup(&path, |file| {
            context.store.create_backup(
                &context.blobs,
                &Keystore::open(&context.paths.keys),
                file,
                &passphrase,
                env!("CARGO_PKG_VERSION"),
                crate::feed_runner::now_micros(),
            )?;
            Ok(())
        })
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "create encrypted backup",
        source,
    })??;
    state
        .feed_status
        .set_idle("Encrypted backup created", FeedStatusCounts::default());
    Ok(true)
}

#[tauri::command]
pub(crate) async fn preview_backup(
    window: Window,
    state: State<'_, AppState>,
    passphrase: String,
    request_id: String,
) -> Result<Option<ReviewPreview>> {
    let passphrase = Zeroizing::new(passphrase);
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(BackupError::Format.into());
    }
    if passphrase.len() > 4096 {
        return Err(BackupError::Passphrase.into());
    }
    let context = state.inner().clone();
    // Register ownership before dispatching filesystem work. A slow eligibility
    // check must never reopen a review cancelled by a later UI action.
    let previous = context
        .backup
        .lock()
        .expect("backup review lock")
        .begin(request_id.clone());
    let guard = context.clone();
    tauri::async_runtime::spawn_blocking(move || {
        drop(previous);
        if crate::backup::pending(&guard.paths)
            || !crate::backup::eligible(&guard.store, &guard.paths)?
        {
            return Err(BackupError::NotEmpty.into());
        }
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "begin backup review",
        source,
    })??;
    if context
        .backup
        .lock()
        .expect("backup review lock")
        .request
        .as_deref()
        != Some(&request_id)
    {
        return Ok(None);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Restore encrypted backup")
        .add_filter("Encrypted Carrel backup", &["carrel-backup"])
        .pick_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver
        .await
        .map_err(|_| AppError::Config("Backup file selection closed.".into()))?
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|_| BackupError::Format)?;
    tauri::async_runtime::spawn_blocking(move || {
        if context
            .backup
            .lock()
            .expect("backup review lock")
            .request
            .as_deref()
            != Some(&request_id)
        {
            return Ok(None);
        }
        let archive = carrel_store::backup::open_backup(&path, &passphrase, &context.paths.root)?;
        let mut review = context.backup.lock().expect("backup review lock");
        Ok(review
            .accept(&request_id, archive)
            .map(|preview| ReviewPreview {
                date: super::sharing::date(preview.created_at_micros),
                preview,
            }))
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "validate encrypted backup",
        source,
    })?
}

#[tauri::command]
pub(crate) async fn cancel_backup_review(
    state: State<'_, AppState>,
    request_id: String,
) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let archive = context
            .backup
            .lock()
            .expect("backup review lock")
            .cancel(&request_id);
        drop(archive);
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "cancel backup review",
        source,
    })?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn restore_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let archive = context
            .backup
            .lock()
            .expect("backup review lock")
            .take(&request_id)?;
        crate::backup::prepare(&context.paths, &context.store, archive)
    })
    .await
    .map_err(|source| AppError::BackgroundTask {
        operation: "prepare archive restore",
        source,
    })??;
    crate::close::request_restore_restart(&app)
}

#[tauri::command]
pub(crate) fn restart_prepared_restore(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    if !crate::backup::pending(&state.paths) {
        return Err(BackupError::Format.into());
    }
    crate::close::request_restore_restart(&app)
}

#[tauri::command]
pub(crate) async fn cancel_pending_restore(state: State<'_, AppState>) -> Result<()> {
    let context = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::backup::cancel_prepared(&context.paths))
        .await
        .map_err(|source| AppError::BackgroundTask {
            operation: "cancel prepared restore",
            source,
        })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::{
        identity::{DeviceCert, Keypair, RevocationCert},
        sharing::ContactCard,
    };
    fn card(master: &Keypair, device: &Keypair, address_key: &Keypair) -> ContactCard {
        ContactCard::sign(
            DeviceCert::new(master, device.public(), Some("Recovered".into())),
            serde_json::to_string(&iroh::EndpointAddr::new(
                iroh::PublicKey::from_bytes(address_key.public().as_bytes()).unwrap(),
            ))
            .unwrap(),
            device,
        )
        .unwrap()
    }
    #[test]
    fn reviewing_recovery_codes_requires_the_same_root_and_matching_endpoint_without_writes() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let master = Keypair::generate();
        let local = Keypair::generate();
        let fresh = Keypair::generate();
        let outsider = Keypair::generate();
        store
            .bind_self_identity(&DeviceCert::new(&master, local.public(), None), &local)
            .unwrap();
        let valid = card(&master, &fresh, &fresh);
        let text = valid.to_text().unwrap();
        assert!(recovered_card(&store, &text).is_ok());
        assert!(!store.self_device_authorized(fresh.public()).unwrap());
        assert!(store.private_peer_addresses().unwrap().is_empty());
        assert!(store.social_contacts().unwrap().is_empty());
        assert!(
            recovered_card(&store, &card(&outsider, &fresh, &fresh).to_text().unwrap()).is_err()
        );
        assert!(
            recovered_card(&store, &card(&master, &fresh, &outsider).to_text().unwrap()).is_err()
        );
        assert!(recovered_card(&store, &card(&master, &local, &local).to_text().unwrap()).is_err());
        let master_as_device = card(&master, &master, &master);
        assert!(recovered_card(&store, &master_as_device.to_text().unwrap()).is_err());
        assert!(store.register_recovered_desktop(&master_as_device).is_err());
        let mut forged = valid.clone();
        forged.address = "changed".into();
        assert!(recovered_card(&store, &forged.to_text().unwrap_or_default()).is_err());
        store.register_recovered_desktop(&valid).unwrap();
        assert!(store.self_device_authorized(fresh.public()).unwrap());
        store
            .accept_self_revocation(&RevocationCert::new(&master, fresh.public()))
            .unwrap();
        assert!(recovered_card(&store, &text).is_err());
        assert!(store.register_recovered_desktop(&valid).is_err());
        assert_eq!(
            store.self_identity().unwrap().unwrap().device_pubkey,
            *local.public()
        );
    }
}
