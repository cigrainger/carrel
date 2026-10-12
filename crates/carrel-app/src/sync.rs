//! Desktop setup and lifecycle; signed facts and network semantics stay in the engine.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use carrel_core::identity::{DeviceCert, Keypair, PublicKey, RevocationCert};
use carrel_core::private_library::BlobReference;
use carrel_store::{
    blobs::BlobId,
    keystore::{Keystore, KeystoreError},
};
use carrel_sync::sharing::runtime::SocialSyncWorker;
use carrel_sync::{
    bridge::SelfBridge,
    pairing::{PairingInvitation, PairingSession},
    runtime::SelfSyncWorker,
    transport::SelfTransport,
};
use serde::Serialize;
use tauri::State;
use zeroize::Zeroizing;

use crate::{AppError, Result, state::AppState};

#[derive(Default)]
pub(crate) struct SyncState {
    worker: Option<SelfSyncWorker>,
    invitation: Option<PairingInvitation>,
    error: Option<String>,
    seen_revision: u64,
    capture_after: Option<String>,
    pub(crate) social: Option<SocialSyncWorker>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSnapshot {
    enabled: bool,
    active: bool,
    can_invite: bool,
    revision: u64,
    error: Option<String>,
    devices: Vec<DeviceSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSummary {
    key: String,
    name: String,
    local: bool,
    revoked: bool,
}

#[tauri::command]
pub(crate) async fn sync_status(state: State<'_, AppState>) -> Result<SyncSnapshot> {
    let sync = state.sync.lock().await;
    let binding = state.store.self_identity()?;
    let authorized = binding.is_some_and(|binding| {
        state
            .store
            .self_device_authorized(&binding.device_pubkey)
            .unwrap_or(false)
    });
    let devices = if let Some(binding) = binding {
        state
            .store
            .self_devices()?
            .into_iter()
            .map(|row| DeviceSummary {
                key: row.certificate.device_pubkey.to_hex(),
                name: row
                    .certificate
                    .device_name
                    .unwrap_or_else(|| "Desktop".into()),
                local: row.certificate.device_pubkey == binding.device_pubkey,
                revoked: row.revocation.is_some() || row.locally_blocked,
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(SyncSnapshot {
        enabled: binding.is_some(),
        active: sync.worker.is_some() && authorized,
        can_invite: master_available(&Keystore::open(&state.paths.keys))?
            && (binding.is_none() || authorized),
        revision: sync
            .worker
            .as_ref()
            .map_or(0, SelfSyncWorker::revision)
            .saturating_add(state.store.social_revision()),
        error: sync.error.clone(),
        devices,
    })
}

#[tauri::command]
pub(crate) async fn enable_sync(
    state: State<'_, AppState>,
    passphrase: String,
    name: String,
) -> Result<()> {
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.len() < 8 || name.trim().is_empty() || name.len() > 256 {
        return Err(AppError::Config(
            "Enter a desktop name and a passphrase of at least eight characters.".into(),
        ));
    }
    let mut sync = state.sync.lock().await;
    let worker = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = worker
            .annotation_lock
            .lock()
            .expect("annotation setup lock");
        let device = crate::commands::highlights::load_device_key(&worker.paths)?;
        let keys = Keystore::open(&worker.paths.keys);
        let master = if master_available(&keys)? {
            keys.load_master(&passphrase)?
        } else {
            let master = Keypair::generate();
            keys.create_master(&master, &passphrase)?;
            master
        };
        if let Some(binding) = worker.store.self_identity()? {
            if binding.master_pubkey != *master.public()
                || binding.device_pubkey != *device.public()
            {
                return Err(AppError::Config(
                    "This installation already uses a different reading identity.".into(),
                ));
            }
        } else {
            bind_local_identity(&worker, &master, &device, name.trim())?;
        }
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|_| AppError::Config("Identity setup could not finish.".into()))??;
    ensure_worker(&state, &mut sync).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn create_sync_invitation(
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<String> {
    let passphrase = Zeroizing::new(passphrase);
    let mut sync = state.sync.lock().await;
    ensure_worker(&state, &mut sync).await?;
    let keys = Keystore::open(&state.paths.keys);
    let master = tauri::async_runtime::spawn_blocking(move || keys.load_master(&passphrase))
        .await
        .map_err(|_| AppError::Config("Could not unlock the reading identity.".into()))??;
    if let Some(previous) = sync.invitation.take() {
        previous.cancel().await;
    }
    let invitation = PairingInvitation::create(
        sync.worker.as_ref().expect("worker started").bridge.clone(),
        master,
    )
    .await?;
    let text = invitation.text.clone();
    sync.invitation = Some(invitation);
    Ok(text)
}

#[tauri::command]
pub(crate) async fn cancel_sync_invitation(state: State<'_, AppState>) -> Result<()> {
    if let Some(invitation) = state.sync.lock().await.invitation.take() {
        invitation.cancel().await;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn join_sync_invitation(
    state: State<'_, AppState>,
    invitation: String,
    name: String,
) -> Result<()> {
    join(&state, invitation, name).await
}

async fn join(state: &AppState, invitation: String, name: String) -> Result<()> {
    let invitation = Zeroizing::new(invitation);
    let mut sync = state.sync.lock().await;
    if state.store.self_identity()?.is_some()
        || master_available(&Keystore::open(&state.paths.keys))?
    {
        return Err(AppError::Config("This installation already has a reading identity. Use a separate Carrel profile to pair another identity.".into()));
    }
    if name.trim().is_empty() || name.len() > 256 {
        return Err(AppError::Config("Enter a desktop name.".into()));
    }
    let device = {
        let _guard = state.annotation_lock.lock().expect("annotation setup lock");
        crate::commands::highlights::load_device_key(&state.paths)?
    };
    let session = join_session(&invitation, &device, name.trim().into()).await?;
    // Pasting and submitting is the user's explicit choice of this Root. No
    // received certificate can otherwise bind an installation automatically.
    state
        .store
        .bind_paired_identity(&session.grant.local_setup()?, &device)?;
    let host = Arc::new(start_host(state, &device).await?);
    let bridge = Arc::new(
        SelfBridge::import(
            state.store.clone(),
            state.blobs.clone(),
            host,
            session.grant.ticket.capability.clone(),
        )
        .await?,
    );
    let worker = SelfSyncWorker::start(bridge.clone()).await?;
    sync.worker = Some(worker);
    sync.error = None;
    // A lost confirmation leaves a durable identity, document and retry queue.
    // The new desktop already knows how to contact the inviter on restart.
    session.confirm(bridge.address()).await?;
    prepare_source(state, &device)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn revoke_sync_device(
    state: State<'_, AppState>,
    key: String,
    passphrase: String,
) -> Result<()> {
    let passphrase = Zeroizing::new(passphrase);
    let device =
        PublicKey::from_hex(&key).map_err(|_| AppError::Config("Invalid desktop key.".into()))?;
    let sync = state.sync.lock().await;
    let binding = state
        .store
        .self_identity()?
        .ok_or(carrel_sync::SyncError::IdentityUnbound)?;
    if device == binding.device_pubkey {
        return Err(AppError::Config(
            "Remove this desktop from another paired desktop.".into(),
        ));
    }
    if !state
        .store
        .self_devices()?
        .iter()
        .any(|row| row.certificate.device_pubkey == device)
    {
        return Err(AppError::Config("Unknown desktop.".into()));
    }
    let keys = Keystore::open(&state.paths.keys);
    let master = tauri::async_runtime::spawn_blocking(move || keys.load_master(&passphrase))
        .await
        .map_err(|_| AppError::Config("Could not unlock the reading identity.".into()))??;
    if *master.public() != binding.master_pubkey {
        return Err(AppError::Config(
            "The recovery key belongs to a different reading identity.".into(),
        ));
    }
    state
        .store
        .accept_self_revocation(&RevocationCert::new(&master, &device))?;
    if let Some(worker) = &sync.worker {
        worker.bridge.refresh_authorization().await?;
    }
    Ok(())
}

pub(crate) async fn ensure_worker(state: &AppState, sync: &mut SyncState) -> Result<()> {
    if sync.worker.is_some() {
        return Ok(());
    }
    let device = Keystore::open(&state.paths.keys).load_device()?;
    let worker = state.clone();
    let seed = Zeroizing::new(device.to_seed_bytes());
    tauri::async_runtime::spawn_blocking(move || {
        prepare_source(&worker, &Keypair::from_seed(&seed))
    })
    .await
    .map_err(|_| AppError::Config("Could not prepare the local reading library.".into()))??;
    let host = Arc::new(start_host(state, &device).await?);
    let bridge =
        Arc::new(SelfBridge::resume(state.store.clone(), state.blobs.clone(), host).await?);
    if let Some(service) = bridge.sharing() {
        sync.social = Some(SocialSyncWorker::start(service));
    }
    sync.worker = Some(SelfSyncWorker::start(bridge).await?);
    sync.error = None;
    Ok(())
}

fn prepare_source(state: &AppState, device: &Keypair) -> Result<()> {
    state.store.adopt_private_state(now_micros(), device)?;
    state.store.seed_existing_private_facts()?;
    state.store.rebuild_private_fact_outbox()?;
    state.store.restore_private_blob_queue()?;
    Ok(())
}

fn bind_local_identity(
    state: &AppState,
    master: &Keypair,
    device: &Keypair,
    name: &str,
) -> Result<()> {
    let certificate = state
        .store
        .device_certificate(master.public(), device.public())?
        .unwrap_or_else(|| DeviceCert::new(master, device.public(), Some(name.into())));
    state.store.bind_self_identity(&certificate, device)?;
    Ok(())
}

pub(crate) fn spawn(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            let mut sync = state.sync.lock().await;
            if state.store.self_identity().ok().flatten().is_some() {
                if let Err(error) = ensure_worker(&state, &mut sync).await {
                    sync.error = Some(error.to_string());
                }
                if sync.worker.is_some() {
                    let worker = state.clone();
                    let after = sync.capture_after.take();
                    match tauri::async_runtime::spawn_blocking(move || {
                        capture_articles(&worker, after)
                    })
                    .await
                    {
                        Ok(Ok(after)) => sync.capture_after = after,
                        _ => {
                            sync.error =
                                Some("Some cached articles could not be prepared for sync.".into())
                        }
                    }
                    let revision = sync
                        .worker
                        .as_ref()
                        .map_or(0, SelfSyncWorker::revision)
                        .saturating_add(state.store.social_revision());
                    if revision != sync.seen_revision {
                        sync.seen_revision = revision;
                        crate::background::spawn_search_rebuild(state.clone());
                    }
                }
            }
            if sync
                .invitation
                .as_ref()
                .is_some_and(PairingInvitation::finished)
            {
                sync.invitation.take();
            }
            drop(sync);
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

fn capture_articles(state: &AppState, after: Option<String>) -> Result<Option<String>> {
    let device = crate::commands::highlights::load_device_key(&state.paths)?;
    let ids = state.store.dirty_private_items(after.as_deref(), 50)?;
    let next = ids.last().cloned();
    for id in ids {
        if capture_article(state, &id, &device).is_err() {
            tracing::warn!(item_id = %id, "saved article unavailable for sync");
        }
    }
    Ok(next)
}

fn capture_article(state: &AppState, id: &str, device: &Keypair) -> Result<()> {
    let Some(capture) = state.store.capture_private_item(id)? else {
        return Ok(());
    };
    let mut resources = BTreeMap::new();
    if let Some(images) = &capture.item.current_images {
        for image in images {
            resources.insert(image.blob_id.clone(), image.clone());
        }
    }
    for head in state.store.private_item_heads(id)? {
        for reference in head.item.resources {
            resources.insert(reference.blob_id.clone(), reference);
        }
    }
    for content in &capture.item.contents {
        if let Ok(blob) = content.blob.blob_id.parse::<BlobId>()
            && let Ok(bytes) = state.blobs.get_blocking(&blob)
            && let Ok(html) = std::str::from_utf8(&bytes)
        {
            collect_resources(state, html, &mut resources);
        }
    }
    for highlight in state.store.list_item_highlights(id)? {
        if let carrel_core::annotation::HighlightLocation::HtmlText(selector) = highlight.location
            && let Some(raw) = selector.content_blob_id
            && let Ok(blob) = raw.parse::<BlobId>()
            && let Ok(bytes) = state.blobs.get_blocking(&blob)
        {
            resources.insert(
                raw.clone(),
                BlobReference {
                    blob_id: raw,
                    byte_size: bytes.len() as u64,
                },
            );
            if let Ok(html) = std::str::from_utf8(&bytes) {
                collect_resources(state, html, &mut resources);
            }
        }
    }
    state.store.adopt_private_item(
        capture,
        resources.into_values().collect(),
        now_micros(),
        device,
    )?;
    Ok(())
}

fn collect_resources(
    state: &AppState,
    html: &str,
    references: &mut BTreeMap<String, BlobReference>,
) {
    // The cached HTML uses content-addressed blob URIs. Only existing, verified
    // files can become references; arbitrary remote URLs never trigger a fetch.
    for prefix in ["blob://", "carrel-blob://"] {
        for tail in html.split(prefix).skip(1) {
            let Some(raw) = tail.get(..64) else {
                continue;
            };
            if let Ok(blob) = raw.parse::<BlobId>()
                && let Ok(bytes) = state.blobs.get_blocking(&blob)
            {
                references.insert(
                    raw.to_owned(),
                    BlobReference {
                        blob_id: raw.into(),
                        byte_size: bytes.len() as u64,
                    },
                );
            }
        }
    }
}

pub(crate) async fn shutdown(state: &AppState) {
    let mut sync = state.sync.lock().await;
    if let Some(worker) = sync.social.take() {
        worker.stop();
    }
    if let Some(invitation) = sync.invitation.take() {
        invitation.cancel().await;
    }
    if let Some(worker) = sync.worker.take() {
        worker.stop().await;
    }
}

pub(crate) async fn refresh_sharing_authorization(state: &AppState) -> Result<()> {
    let sync = state.sync.lock().await;
    if let Some(worker) = &sync.worker {
        worker.bridge.refresh_authorization().await?;
    }
    Ok(())
}

fn master_available(keys: &Keystore) -> Result<bool> {
    match keys.master_public_key() {
        Ok(_) => Ok(true),
        Err(KeystoreError::ReadKeyFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

fn now_micros() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64
}

async fn start_host(state: &AppState, device: &Keypair) -> carrel_sync::Result<SelfTransport> {
    #[cfg(test)]
    {
        SelfTransport::start_with_sharing_builder(
            state.store.clone(),
            &state.paths.root.join("sync"),
            device,
            state.blobs.clone(),
            iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
    }
    #[cfg(not(test))]
    {
        SelfTransport::start_with_sharing(
            state.store.clone(),
            &state.paths.root.join("sync"),
            device,
            state.blobs.clone(),
        )
        .await
    }
}

async fn join_session(
    text: &str,
    device: &Keypair,
    name: String,
) -> carrel_sync::Result<PairingSession> {
    #[cfg(test)]
    {
        PairingSession::with_builder(
            text,
            device,
            name,
            iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
    }
    #[cfg(not(test))]
    {
        PairingSession::join(text, device, name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::private_state::{StateKey, StateKind, StateValue};
    use carrel_store::{Store, blobs::BlobStore};
    use cozo::DataValue;

    fn profile(path: &std::path::Path) -> AppState {
        let paths = crate::config::InstallPaths {
            root: path.into(),
            store: path.join("store"),
            blobs: path.join("blobs"),
            keys: path.join("keys"),
            keymap_config: path.join("keymap.toml"),
        };
        let store = Store::open(&paths.store).unwrap();
        store.migrate().unwrap();
        AppState::new(store, BlobStore::open(&paths.blobs), paths)
    }

    async fn until(ready: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while !ready() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("native profiles should converge through the background worker");
    }

    #[tokio::test]
    async fn native_profiles_pair_transfer_cached_library_and_resume_without_a_master_seed_on_the_second_desktop()
     {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("carrel_sync=debug")
            .try_init();
        let dir = tempfile::tempdir().unwrap();
        let a = profile(&dir.path().join("a"));
        let b = profile(&dir.path().join("b"));
        let master = Keypair::generate();
        let akey = crate::commands::highlights::load_device_key(&a.paths).unwrap();
        Keystore::open(&a.paths.keys)
            .create_master(&master, "Synthetic fixture passphrase")
            .unwrap();
        a.store
            .bind_self_identity(
                &DeviceCert::new(&master, akey.public(), Some("A".into())),
                &akey,
            )
            .unwrap();
        let image = a.blobs.put_blocking(b"native fixture image").unwrap();
        let original = format!("<p>Café 文</p><img src=\"blob://{image}\"><script>bad()</script>");
        let body = a.blobs.put_blocking(original.as_bytes()).unwrap();
        a.store.query_with_params(r#"
            { ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [['article', 'article', 'Café 文', [], null, null, 'fr', null, validity(1)]]
              :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at} }
            { ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <-
                [['article', 'html_readable', $blob, validity(1), 'fixture', $size]]
              :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size} }
        "#, BTreeMap::from([("blob".into(), DataValue::from(body.to_string())), ("size".into(), DataValue::from(original.len() as i64))])).unwrap();
        {
            let mut state = a.sync.lock().await;
            ensure_worker(&a, &mut state).await.unwrap();
        }
        capture_articles(&a, None).unwrap();
        let bridge = a.sync.lock().await.worker.as_ref().unwrap().bridge.clone();
        let invitation = PairingInvitation::with_builder(
            bridge.clone(),
            master,
            iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap();
        join(&b, invitation.text.clone(), "B".into()).await.unwrap();
        let bkey = Keystore::open(&b.paths.keys).load_device().unwrap();
        assert_ne!(akey.public(), bkey.public());
        assert!(!master_available(&Keystore::open(&b.paths.keys)).unwrap());
        until(|| {
            b.store.get_item_detail("article").unwrap().is_some()
                && b.blobs.get_blocking(&body).is_ok()
                && b.blobs.get_blocking(&image).is_ok()
        })
        .await;
        assert_eq!(
            b.blobs.get_blocking(&body).unwrap().as_ref(),
            original.as_bytes()
        );
        assert!(!carrel_feeds::sanitize_cached_html(&original).contains("<script"));
        a.store
            .write_private_state(
                StateKey {
                    kind: StateKind::Star,
                    target: "article".into(),
                },
                StateValue::Star(true),
                2,
                &akey,
            )
            .unwrap();
        let star_key = StateKey {
            kind: StateKind::Star,
            target: "article".into(),
        };
        until(|| b.store.private_state_value(&star_key).unwrap() == Some(StateValue::Star(true)))
            .await;
        assert!(b.sync.lock().await.worker.as_ref().unwrap().revision() > 0);
        let document = b.store.private_document().unwrap().unwrap();
        shutdown(&b).await;
        {
            let mut state = b.sync.lock().await;
            ensure_worker(&b, &mut state).await.unwrap();
        }
        assert_eq!(b.store.private_document().unwrap().unwrap(), document);
        assert_eq!(
            b.sync
                .lock()
                .await
                .worker
                .as_ref()
                .unwrap()
                .bridge
                .document()
                .id()
                .as_bytes(),
            &document.namespace
        );
        a.store
            .write_private_state(
                StateKey {
                    kind: StateKind::Star,
                    target: "article".into(),
                },
                StateValue::Star(false),
                3,
                &akey,
            )
            .unwrap();
        until(|| b.store.private_state_value(&star_key).unwrap() == Some(StateValue::Star(false)))
            .await;
        shutdown(&b).await;
        drop(invitation);
        drop(bridge);
        shutdown(&a).await;
    }
    #[test]
    fn setup_preserves_an_existing_legacy_certificate_and_operational_key() {
        let dir = tempfile::tempdir().unwrap();
        let state = profile(dir.path());
        let master = Keypair::generate();
        let device = crate::commands::highlights::load_device_key(&state.paths).unwrap();
        let certificate =
            DeviceCert::new_at_micros(&master, device.public(), 1, Some("Original name".into()));
        state.store.query_with_params("?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]] :put device_certificate {master_pubkey, device_pubkey => device_cert}",
                BTreeMap::from([("master".into(),DataValue::Bytes(master.public().as_bytes().to_vec())),("device".into(),DataValue::Bytes(device.public().as_bytes().to_vec())),("cert".into(),DataValue::Bytes(certificate.to_bytes().unwrap()))])).unwrap();
        assert!(state.store.self_identity().unwrap().is_none());
        bind_local_identity(&state, &master, &device, "New label").unwrap();
        assert_eq!(
            state.store.self_devices().unwrap()[0].certificate,
            certificate
        );
        assert_eq!(
            Keystore::open(&state.paths.keys)
                .load_device()
                .unwrap()
                .public(),
            device.public()
        );
    }
}
