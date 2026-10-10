//! Highlight and note commands for the desktop shell.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use carrel_core::annotation::{
    AnnotationCursor, AnnotationPage, HighlightLocation, normalize_readable_text,
};
use carrel_core::identity::{Keypair, PublicKey};
use carrel_store::blobs::{BlobError, BlobId, BlobStore};
use carrel_store::highlights::{
    AnnotationReview, EditNoteInput, HighlightRecord, NoteRecord, PutHighlightInput, PutNoteInput,
    ResolveNoteInput,
};
use carrel_store::items::ItemDetail as StoreItemDetail;
use carrel_store::keystore::{Keystore, KeystoreError};
use kuchiki::traits::TendrilSink;
use serde::{Deserialize, Serialize};
use tauri::State;
use time::OffsetDateTime;

use crate::commands::items::source_name;
use crate::error::AppError;
use crate::state::AppState;
use crate::{Result, config::InstallPaths};

const DEFAULT_HIGHLIGHT_LIMIT: usize = 100;
const MAX_HIGHLIGHT_LIMIT: usize = 100;

/// Create a highlight for an item.
#[tauri::command]
pub fn create_highlight(
    state: State<'_, AppState>,
    request: CreateHighlightRequest,
) -> Result<HighlightDetail> {
    let _guard = state
        .annotation_lock
        .lock()
        .expect("annotation command lock");
    let device = load_device_key(&state.paths)?;
    create_highlight_with_device(&state.store, &state.blobs, &device, request)
}

/// List highlights for an item.
#[tauri::command]
pub fn list_item_highlights(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<HighlightDetail>> {
    require_item(&state.store, &item_id)?;
    state
        .store
        .list_item_highlights(&item_id)?
        .iter()
        .map(HighlightDetail::from_record)
        .collect()
}

/// List highlights across the library.
#[tauri::command]
pub fn list_highlights(
    state: State<'_, AppState>,
    limit: Option<usize>,
    after: Option<AnnotationCursor>,
) -> Result<AnnotationPage<HighlightListRow>> {
    let device = {
        let _guard = state
            .annotation_lock
            .lock()
            .expect("annotation command lock");
        existing_device_key(&state.paths)?
    };
    list_highlights_from_store(
        &state.store,
        limit,
        after.as_ref(),
        device.as_ref().map(Keypair::public),
    )
}

pub(crate) fn list_highlights_from_store(
    store: &carrel_store::Store,
    limit: Option<usize>,
    after: Option<&AnnotationCursor>,
    device: Option<&PublicKey>,
) -> Result<AnnotationPage<HighlightListRow>> {
    let limit = limit
        .unwrap_or(DEFAULT_HIGHLIGHT_LIMIT)
        .clamp(1, MAX_HIGHLIGHT_LIMIT);
    let page = store.annotation_page(limit, after)?;
    let permissions = device
        .map(|device| store.annotation_creator_permissions(device))
        .transpose()?
        .unwrap_or_default();
    let device_names = if store.self_identity()?.is_some() {
        store.self_devices()?
    } else {
        Vec::new()
    }
    .into_iter()
    .filter_map(|entry| {
        entry
            .certificate
            .device_name
            .map(|name| (entry.certificate.device_pubkey, name))
    })
    .collect();
    Ok(AnnotationPage {
        items: page
            .items
            .iter()
            .map(|row| HighlightListRow::from_review(row, &permissions, &device_names, device))
            .collect::<Result<_>>()?,
        next_cursor: page.next_cursor,
    })
}

/// Read every current version directly for stale-editor recovery.
#[tauri::command]
pub fn review_highlight_note(state: State<'_, AppState>, note_id: String) -> Result<NoteDetail> {
    let device = {
        let _guard = state
            .annotation_lock
            .lock()
            .expect("annotation command lock");
        existing_device_key(&state.paths)?
    };
    review_note_from_store(&state.store, &note_id, device.as_ref().map(Keypair::public))
}

pub(crate) fn review_note_from_store(
    store: &carrel_store::Store,
    note_id: &str,
    device: Option<&PublicKey>,
) -> Result<NoteDetail> {
    let heads = store.note_heads(note_id)?;
    let first = heads
        .first()
        .ok_or_else(|| carrel_store::StoreError::AnnotationUnavailable { id: note_id.into() })?;
    if first.target_kind.as_deref() != Some("highlight") {
        return Err(AppError::InvalidData {
            context: "note.target",
            value: "expected highlight note".into(),
        });
    }
    let permissions = device
        .map(|key| store.annotation_creator_permissions(key))
        .transpose()?
        .unwrap_or_default();
    let names = if store.self_identity()?.is_some() {
        store.self_devices()?
    } else {
        Vec::new()
    }
    .into_iter()
    .filter_map(|entry| {
        entry
            .certificate
            .device_name
            .map(|name| (entry.certificate.device_pubkey, name))
    })
    .collect();
    Ok(
        NoteDetail::from_record(first, permissions.contains(&first.created_by))
            .with_conflicts(&heads, &names, device),
    )
}

/// Create a note attached to a highlight.
#[tauri::command]
pub fn create_highlight_note(
    state: State<'_, AppState>,
    request: CreateHighlightNoteRequest,
) -> Result<NoteDetail> {
    let _guard = state
        .annotation_lock
        .lock()
        .expect("annotation command lock");
    let device = load_device_key(&state.paths)?;
    create_highlight_note_with_device(&state.store, &device, request)
}

/// Save a note against the revision the editor originally read.
#[tauri::command]
pub fn edit_highlight_note(
    state: State<'_, AppState>,
    request: EditHighlightNoteRequest,
) -> Result<NoteDetail> {
    let _guard = state
        .annotation_lock
        .lock()
        .expect("annotation command lock");
    let device = load_device_key(&state.paths)?;
    let previous = state.store.get_note(&request.note_id)?.ok_or_else(|| {
        carrel_store::StoreError::AnnotationUnavailable {
            id: request.note_id.clone(),
        }
    })?;
    if previous.target_kind.as_deref() != Some("highlight") {
        return Err(AppError::InvalidData {
            context: "note.target",
            value: "expected highlight note".into(),
        });
    }
    let record = state.store.edit_note(
        EditNoteInput {
            id: request.note_id,
            body: request.body,
            expected_revision_id: request.expected_revision_id,
            now_micros: None,
        },
        &device,
    )?;
    Ok(NoteDetail::from_record(&record, true))
}

/// Resolve exactly the versions reviewed by the editor, preserving later arrivals.
#[tauri::command]
pub fn resolve_highlight_note(
    state: State<'_, AppState>,
    request: ResolveHighlightNoteRequest,
) -> Result<NoteDetail> {
    let _guard = state
        .annotation_lock
        .lock()
        .expect("annotation command lock");
    let device = load_device_key(&state.paths)?;
    let heads = state.store.note_heads(&request.note_id)?;
    if heads
        .iter()
        .any(|note| note.target_kind.as_deref() != Some("highlight"))
    {
        return Err(AppError::InvalidData {
            context: "note.target",
            value: "expected highlight note".into(),
        });
    }
    let record = state.store.resolve_note(
        ResolveNoteInput {
            id: request.note_id,
            body: request.body,
            expected_revision_ids: request.expected_revision_ids,
            now_micros: None,
        },
        &device,
    )?;
    Ok(NoteDetail::from_record(&record, true))
}

/// Remove a highlight and hide its attached notes from review and export.
#[tauri::command]
pub fn remove_highlight(state: State<'_, AppState>, highlight_id: String) -> Result<()> {
    let _guard = state
        .annotation_lock
        .lock()
        .expect("annotation command lock");
    let device = load_device_key(&state.paths)?;
    state.store.remove_highlight(&highlight_id, &device)?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditHighlightNoteRequest {
    /// Existing note identity.
    pub note_id: String,
    /// Replacement Markdown body.
    pub body: String,
    /// Revision originally displayed to the editor.
    pub expected_revision_id: String,
}

/// Exact set of current versions acknowledged by conflict review.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveHighlightNoteRequest {
    pub note_id: String,
    pub body: String,
    pub expected_revision_ids: Vec<String>,
}

/// Highlight creation request accepted by the webview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateHighlightRequest {
    /// Item being highlighted.
    pub item_id: String,
    /// Text selector location.
    pub location: HighlightLocation,
    /// Optional display label for the location.
    pub location_label: Option<String>,
    /// Optional color token.
    pub color: Option<String>,
}

/// Highlight note creation request accepted by the webview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateHighlightNoteRequest {
    /// Highlight receiving the note.
    pub highlight_id: String,
    /// Note body.
    pub body: String,
}

/// Highlight DTO returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightDetail {
    /// Stable highlight id.
    pub id: String,
    /// Item being highlighted.
    pub item_id: String,
    /// Exact selected text.
    pub quoted_text: String,
    /// Text selector location.
    pub location: HighlightLocation,
    /// Optional display label for the location.
    pub location_label: Option<String>,
    /// Optional color token.
    pub color: Option<String>,
    /// Hex-encoded device public key.
    pub created_by: String,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
    /// Creation time in Unix microseconds.
    pub created_at_micros: i64,
    /// Hex-encoded Ed25519 signature.
    pub signature: String,
}

/// Library highlight row with item context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightListRow {
    /// Highlight fact.
    pub highlight: HighlightDetail,
    /// Current item title.
    pub item_title: String,
    /// Current item source label.
    pub item_source_name: String,
    /// Current item URL, if known.
    pub item_primary_url: Option<String>,
    /// Excerpt from the newest note attached to this highlight, if any.
    pub note_excerpt: Option<String>,
    /// Every current note, without excerpt truncation.
    pub notes: Vec<NoteDetail>,
    /// Whether this device may remove the private highlight.
    pub can_remove: bool,
}

/// Note DTO returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    /// Stable note id.
    pub id: String,
    /// Signed revision identity; clocks never determine the winning draft.
    pub revision_id: String,
    /// Immediate predecessors superseded by this revision.
    pub superseded_revision_ids: Vec<String>,
    /// Every unresolved current version, including the displayed representative.
    pub conflicts: Vec<NoteConflictVersion>,
    /// Note body.
    pub body: String,
    /// Optional target kind.
    pub target_kind: Option<String>,
    /// Optional target id.
    pub target_id: Option<String>,
    /// Hex-encoded device public key.
    pub created_by: String,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
    /// Creation time in Unix microseconds.
    pub created_at_micros: i64,
    /// Last update time in Unix microseconds.
    pub updated_at_micros: i64,
    /// Hex-encoded Ed25519 signature.
    pub signature: String,
    /// Whether this device may edit the private note.
    pub can_edit: bool,
}

/// Complete version shown during conflict review.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteConflictVersion {
    pub revision_id: String,
    pub body: String,
    pub saved_at: String,
    pub device_label: String,
}

pub(crate) fn create_highlight_with_device(
    store: &carrel_store::Store,
    blobs: &BlobStore,
    device: &Keypair,
    request: CreateHighlightRequest,
) -> Result<HighlightDetail> {
    let item = require_item(store, &request.item_id)?;
    validate_location_against_item(blobs, &item, &request.location)?;
    let record = store.put_highlight(
        PutHighlightInput {
            item_id: request.item_id,
            location: request.location,
            location_label: request.location_label,
            color: request.color,
            created_at_micros: None,
        },
        device,
    )?;

    HighlightDetail::from_record(&record)
}

pub(crate) fn create_highlight_note_with_device(
    store: &carrel_store::Store,
    device: &Keypair,
    request: CreateHighlightNoteRequest,
) -> Result<NoteDetail> {
    if store.get_highlight(&request.highlight_id)?.is_none() {
        return Err(AppError::InvalidData {
            context: "note.highlight_id",
            value: request.highlight_id,
        });
    }

    let record = store.put_note(
        PutNoteInput {
            body: request.body,
            target_kind: Some("highlight".to_string()),
            target_id: Some(request.highlight_id),
            created_at_micros: None,
        },
        device,
    )?;

    Ok(NoteDetail::from_record(&record, true))
}

fn existing_device_key(paths: &InstallPaths) -> Result<Option<Keypair>> {
    match Keystore::open(&paths.keys).load_device() {
        Ok(key) => Ok(Some(key)),
        Err(KeystoreError::ReadKeyFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn load_device_key(paths: &InstallPaths) -> Result<Keypair> {
    load_device_key_from_path(&paths.keys)
}

fn load_device_key_from_path(path: &Path) -> Result<Keypair> {
    let keystore = Keystore::open(path);
    match keystore.load_device() {
        Ok(device) => Ok(device),
        Err(KeystoreError::ReadKeyFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            let device = Keypair::generate();
            match keystore.create_device(&device) {
                Ok(()) => Ok(device),
                Err(KeystoreError::KeyAlreadyExists { .. }) => Ok(keystore.load_device()?),
                Err(error) => Err(error.into()),
            }
        }
        Err(source) => Err(source.into()),
    }
}

fn require_item(store: &carrel_store::Store, item_id: &str) -> Result<StoreItemDetail> {
    store
        .get_item_detail(item_id)?
        .ok_or_else(|| AppError::InvalidData {
            context: "highlight.item_id",
            value: item_id.to_string(),
        })
}

fn validate_location_against_item(
    blobs: &BlobStore,
    item: &StoreItemDetail,
    location: &HighlightLocation,
) -> Result<()> {
    validate_content_blob_reference(item, location)?;

    if let Some(readable_text) = readable_text(blobs, item)? {
        location
            .validate_against_text(&readable_text)
            .map_err(|source| AppError::InvalidData {
                context: "highlight.location",
                value: source.to_string(),
            })?;
    } else {
        location
            .validate()
            .map_err(|source| AppError::InvalidData {
                context: "highlight.location",
                value: source.to_string(),
            })?;
    }

    Ok(())
}

fn validate_content_blob_reference(
    item: &StoreItemDetail,
    location: &HighlightLocation,
) -> Result<()> {
    let HighlightLocation::HtmlText(selector) = location;
    let Some(selector_blob_id) = selector.content_blob_id.as_deref() else {
        return Ok(());
    };
    let Some(readable) = item.readable.as_ref() else {
        return Err(AppError::InvalidData {
            context: "highlight.location.content_blob_id",
            value: selector_blob_id.to_string(),
        });
    };
    if readable.blob_id == selector_blob_id {
        Ok(())
    } else {
        Err(AppError::InvalidData {
            context: "highlight.location.content_blob_id",
            value: format!("expected {}, got {selector_blob_id}", readable.blob_id),
        })
    }
}

fn readable_text(blobs: &BlobStore, item: &StoreItemDetail) -> Result<Option<String>> {
    let Some(readable) = item.readable.as_ref() else {
        return Ok(None);
    };
    let blob_id = BlobId::from_str(&readable.blob_id)?;
    let bytes = match blobs.get_blocking(&blob_id) {
        Ok(bytes) => bytes,
        Err(BlobError::NotFound { .. }) => return Ok(None),
        Err(source) => return Err(source.into()),
    };
    let html = String::from_utf8(bytes.to_vec()).map_err(|source| AppError::InvalidData {
        context: "item_content.blob",
        value: source.to_string(),
    })?;
    let text = kuchiki::parse_html().one(html).text_contents();
    Ok(Some(normalize_readable_text(&text)))
}

fn iso8601_timestamp(micros: i64) -> String {
    let timestamp = OffsetDateTime::from_unix_timestamp(micros / 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let date = timestamp.date();
    let time = timestamp.time();

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year(),
        u8::from(date.month()),
        date.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

impl HighlightDetail {
    fn from_record(record: &HighlightRecord) -> Result<Self> {
        Ok(Self {
            id: record.id.clone(),
            item_id: record.item_id.clone(),
            quoted_text: record.quoted_text.clone(),
            location: record.location.clone(),
            location_label: record.location_label.clone(),
            color: record.color.clone(),
            created_by: record.created_by.to_hex(),
            created_at: iso8601_timestamp(record.created_at_micros),
            created_at_micros: record.created_at_micros,
            signature: record.signature.to_hex(),
        })
    }
}

impl HighlightListRow {
    fn from_review(
        review: &AnnotationReview,
        permissions: &BTreeSet<PublicKey>,
        device_names: &BTreeMap<PublicKey, String>,
        local_device: Option<&PublicKey>,
    ) -> Result<Self> {
        let record = &review.source;
        Ok(Self {
            highlight: HighlightDetail::from_record(&record.highlight)?,
            item_title: record.item_title.clone(),
            item_source_name: source_name(
                record.item_primary_url.as_deref(),
                review.site_name.as_deref(),
            ),
            item_primary_url: record.item_primary_url.clone(),
            note_excerpt: review
                .notes
                .first()
                .and_then(|note| note_excerpt(&note.body)),
            notes: review
                .notes
                .iter()
                .map(|note| {
                    let detail =
                        NoteDetail::from_record(note, permissions.contains(&note.created_by));
                    detail.with_conflicts(
                        review
                            .conflicts
                            .get(&note.id)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                        device_names,
                        local_device,
                    )
                })
                .collect(),
            can_remove: permissions.contains(&record.highlight.created_by),
        })
    }
}

impl NoteDetail {
    fn with_conflicts(
        mut self,
        versions: &[NoteRecord],
        device_names: &BTreeMap<PublicKey, String>,
        local_device: Option<&PublicKey>,
    ) -> Self {
        if versions.len() > 1 {
            self.conflicts = versions
                .iter()
                .map(|version| NoteConflictVersion {
                    revision_id: version.revision_id.clone(),
                    body: version.body.clone(),
                    saved_at: iso8601_timestamp(version.updated_at_micros),
                    device_label: device_names
                        .get(&version.updated_by)
                        .cloned()
                        .unwrap_or_else(|| {
                            if Some(&version.updated_by) == local_device {
                                "This device"
                            } else {
                                "Another device"
                            }
                            .into()
                        }),
                })
                .collect();
        }
        self
    }

    fn from_record(record: &NoteRecord, can_edit: bool) -> Self {
        Self {
            id: record.id.clone(),
            revision_id: record.revision_id.clone(),
            superseded_revision_ids: record.parents.clone(),
            conflicts: Vec::new(),
            body: record.body.clone(),
            target_kind: record.target_kind.clone(),
            target_id: record.target_id.clone(),
            created_by: record.created_by.to_hex(),
            created_at: iso8601_timestamp(record.created_at_micros),
            created_at_micros: record.created_at_micros,
            updated_at_micros: record.updated_at_micros,
            signature: record.signature.to_hex(),
            can_edit,
        }
    }
}

fn note_excerpt(body: &str) -> Option<String> {
    let text = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }

    const MAX_CHARS: usize = 180;
    if text.chars().count() <= MAX_CHARS {
        return Some(text);
    }

    let excerpt = text.chars().take(MAX_CHARS).collect::<String>();
    Some(format!("{excerpt}..."))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use carrel_core::annotation::HtmlTextSelector;
    use cozo::{DataValue, Num, Validity};

    use super::*;

    #[test]
    fn create_highlight_validates_readable_text_and_returns_dto() {
        let (store, _tempdir, blobs, blob_id) = store_with_readable_item();
        let device = Keypair::from_seed(&[13_u8; 32]);

        let detail = create_highlight_with_device(
            &store,
            &blobs,
            &device,
            request_for_blob(blob_id.as_str(), "quiet web", 4, 13),
        )
        .unwrap();

        assert_eq!(detail.item_id, "item-1");
        assert_eq!(detail.quoted_text, "quiet web");
        assert_eq!(detail.created_by.len(), 64);
        assert_eq!(detail.signature.len(), 128);
        assert_eq!(store.list_item_highlights("item-1").unwrap().len(), 1);
    }

    #[test]
    fn create_highlight_rejects_mismatched_readable_selector() {
        let (store, _tempdir, blobs, blob_id) = store_with_readable_item();
        let device = Keypair::from_seed(&[14_u8; 32]);
        let error = create_highlight_with_device(
            &store,
            &blobs,
            &device,
            request_for_blob(blob_id.as_str(), "quiet web", 0, 9),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            AppError::InvalidData {
                context: "highlight.location",
                ..
            }
        ));
    }

    #[test]
    fn create_highlight_rejects_unknown_item() {
        let store = migrated_store();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let device = Keypair::from_seed(&[15_u8; 32]);

        let error = create_highlight_with_device(
            &store,
            &blobs,
            &device,
            request_for_blob("missing", "quiet web", 4, 13),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            AppError::InvalidData {
                context: "highlight.item_id",
                ..
            }
        ));
    }

    #[test]
    fn create_highlight_note_returns_note_dto() {
        let (store, _tempdir, blobs, blob_id) = store_with_readable_item();
        let device = Keypair::from_seed(&[16_u8; 32]);
        let highlight = create_highlight_with_device(
            &store,
            &blobs,
            &device,
            request_for_blob(blob_id.as_str(), "quiet web", 4, 13),
        )
        .unwrap();

        let note = create_highlight_note_with_device(
            &store,
            &device,
            CreateHighlightNoteRequest {
                highlight_id: highlight.id.clone(),
                body: "Follow up later.".to_string(),
            },
        )
        .unwrap();

        assert_eq!(note.target_kind.as_deref(), Some("highlight"));
        assert_eq!(note.target_id.as_deref(), Some(highlight.id.as_str()));
        assert_eq!(note.created_by.len(), 64);
        assert_eq!(
            store
                .list_notes_for_target("highlight", &highlight.id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn list_highlights_returns_source_and_latest_note_context() {
        let (store, _tempdir, blobs, blob_id) = store_with_readable_item();
        let device = Keypair::from_seed(&[18_u8; 32]);
        let highlight = create_highlight_with_device(
            &store,
            &blobs,
            &device,
            request_for_blob(blob_id.as_str(), "quiet web", 4, 13),
        )
        .unwrap();

        store
            .put_note(
                PutNoteInput {
                    body: "Older note.".to_string(),
                    target_kind: Some("highlight".to_string()),
                    target_id: Some(highlight.id.clone()),
                    created_at_micros: Some(1_700_000_000_000_000),
                },
                &device,
            )
            .unwrap();
        store
            .put_note(
                PutNoteInput {
                    body: "Newest note\nwith   spacing.".to_string(),
                    target_kind: Some("highlight".to_string()),
                    target_id: Some(highlight.id.clone()),
                    created_at_micros: Some(1_700_000_001_000_000),
                },
                &device,
            )
            .unwrap();

        let page =
            list_highlights_from_store(&store, Some(10), None, Some(device.public())).unwrap();
        let rows = page.items;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_title, "A Quiet Web");
        assert_eq!(rows[0].item_source_name, "Example");
        assert_eq!(
            rows[0].item_primary_url.as_deref(),
            Some("https://example.com/quiet")
        );
        assert_eq!(
            rows[0].note_excerpt.as_deref(),
            Some("Newest note with spacing.")
        );
        assert_eq!(rows[0].notes.len(), 2);
        assert!(rows[0].can_remove && rows[0].notes.iter().all(|note| note.can_edit));
    }

    #[test]
    fn loads_device_key_from_keystore_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let device = Keypair::from_seed(&[17_u8; 32]);
        Keystore::open(tempdir.path()).save_device(&device).unwrap();

        let loaded = load_device_key_from_path(tempdir.path()).unwrap();

        assert_eq!(loaded.public(), device.public());
    }

    #[test]
    fn annotation_permissions_follow_the_bound_identity_and_revocation() {
        use carrel_core::identity::{DeviceCert, RevocationCert};
        let (store, _tempdir, blobs, blob_id) = store_with_readable_item();
        let creator = Keypair::from_seed(&[44; 32]);
        let local = Keypair::from_seed(&[45; 32]);
        let master = Keypair::from_seed(&[46; 32]);
        let highlight = create_highlight_with_device(
            &store,
            &blobs,
            &creator,
            request_for_blob(blob_id.as_str(), "quiet web", 4, 13),
        )
        .unwrap();
        store
            .put_note(
                PutNoteInput {
                    body: "An existing note".into(),
                    target_kind: Some("highlight".into()),
                    target_id: Some(highlight.id),
                    created_at_micros: Some(1),
                },
                &creator,
            )
            .unwrap();
        let before = list_highlights_from_store(&store, None, None, Some(local.public())).unwrap();
        assert!(!before.items[0].can_remove && !before.items[0].notes[0].can_edit);
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&master, local.public(), 1, None),
                &local,
            )
            .unwrap();
        store
            .accept_self_device_certificate(&DeviceCert::new_at_micros(
                &master,
                creator.public(),
                1,
                None,
            ))
            .unwrap();
        let paired = list_highlights_from_store(&store, None, None, Some(local.public())).unwrap();
        assert!(paired.items[0].can_remove && paired.items[0].notes[0].can_edit);
        store
            .accept_self_revocation(&RevocationCert::new_at_micros(&master, creator.public(), 2))
            .unwrap();
        let preserved =
            list_highlights_from_store(&store, None, None, Some(local.public())).unwrap();
        assert!(preserved.items[0].can_remove && preserved.items[0].notes[0].can_edit);
        store
            .accept_self_revocation(&RevocationCert::new_at_micros(&master, local.public(), 3))
            .unwrap();
        let revoked = list_highlights_from_store(&store, None, None, Some(local.public())).unwrap();
        assert!(!revoked.items[0].can_remove && !revoked.items[0].notes[0].can_edit);
        assert_eq!(revoked.items[0].notes[0].body, "An existing note");
    }

    #[test]
    fn load_device_key_creates_missing_device_key() {
        let tempdir = tempfile::tempdir().unwrap();

        let first = load_device_key_from_path(tempdir.path()).unwrap();
        let second = load_device_key_from_path(tempdir.path()).unwrap();

        assert_eq!(first.public(), second.public());
        assert!(tempdir.path().join("device.key").exists());
    }

    fn store_with_readable_item() -> (carrel_store::Store, tempfile::TempDir, BlobStore, String) {
        let store = migrated_store();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let body = b"<article><p>The quiet web matters.</p></article>";
        let blob_id = blobs.put_blocking(body).unwrap().to_string();
        insert_item(&store, "item-1");
        insert_item_content(&store, "item-1", blob_id.as_str(), body.len());
        (store, tempdir, blobs, blob_id)
    }

    fn migrated_store() -> carrel_store::Store {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
    }

    fn request_for_blob(
        content_blob_id: &str,
        exact: &str,
        start: u32,
        end: u32,
    ) -> CreateHighlightRequest {
        CreateHighlightRequest {
            item_id: "item-1".to_string(),
            location: HighlightLocation::HtmlText(HtmlTextSelector {
                content_blob_id: Some(content_blob_id.to_string()),
                exact: exact.to_string(),
                prefix: "The".to_string(),
                suffix: "matters.".to_string(),
                start,
                end,
            }),
            location_label: None,
            color: Some("yellow".to_string()),
        }
    }

    fn insert_item(store: &carrel_store::Store, id: &str) {
        store
            .query_with_params(
                r#"
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = $id,
                    kind = 'article',
                    title = 'A Quiet Web',
                    creators = [],
                    primary_url = 'https://example.com/quiet',
                    published_at = null,
                    language = 'en',
                    summary = null,
                    discovered_at = $discovered_at
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
                "#,
                BTreeMap::from([
                    ("id".to_string(), DataValue::from(id)),
                    (
                        "discovered_at".to_string(),
                        DataValue::Validity(Validity::from((1_700_000_000_000_000, true))),
                    ),
                ]),
            )
            .unwrap();

        store
            .query_with_params(
                r#"
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = $item_id,
                    feed_url = null,
                    word_count = $word_count,
                    estimated_read_minutes = 1,
                    site_name = 'Example',
                    byline = null
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(id)),
                    ("word_count".to_string(), DataValue::Num(Num::Int(120))),
                ]),
            )
            .unwrap();
    }

    fn insert_item_content(
        store: &carrel_store::Store,
        item_id: &str,
        blob_id: &str,
        byte_size: usize,
    ) {
        store
            .query_with_params(
                r#"
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    item_id = $item_id,
                    format = 'html_readable',
                    blob_id = $blob_id,
                    fetched_at = $fetched_at,
                    extracted_with = 'readability',
                    byte_size = $byte_size
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(item_id)),
                    ("blob_id".to_string(), DataValue::from(blob_id)),
                    (
                        "byte_size".to_string(),
                        DataValue::Num(Num::Int(i64::try_from(byte_size).unwrap())),
                    ),
                    (
                        "fetched_at".to_string(),
                        DataValue::Validity(Validity::from((1_700_000_000_000_000, true))),
                    ),
                ]),
            )
            .unwrap();
    }
}
