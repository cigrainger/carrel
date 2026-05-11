//! Highlight and note commands for the desktop shell.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use carrel_core::annotation::{HighlightLocation, normalize_readable_text};
use carrel_core::identity::Keypair;
use carrel_store::blobs::{BlobError, BlobId, BlobStore};
use carrel_store::highlights::{
    HighlightRecord, HighlightWithItem, NoteRecord, PutHighlightInput, PutNoteInput,
};
use carrel_store::items::ItemDetail as StoreItemDetail;
use carrel_store::keystore::Keystore;
use cozo::DataValue;
use kuchiki::traits::TendrilSink;
use serde::{Deserialize, Serialize};
use tauri::State;
use time::OffsetDateTime;

use crate::commands::items::source_name;
use crate::error::AppError;
use crate::state::AppState;
use crate::{Result, config::InstallPaths};

const DEFAULT_HIGHLIGHT_LIMIT: usize = 100;
const MAX_HIGHLIGHT_LIMIT: usize = 500;

/// Create a highlight for an item.
#[tauri::command]
pub fn create_highlight(
    state: State<'_, AppState>,
    request: CreateHighlightRequest,
) -> Result<HighlightDetail> {
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
) -> Result<Vec<HighlightListRow>> {
    list_highlights_from_store(&state.store, limit)
}

pub(crate) fn list_highlights_from_store(
    store: &carrel_store::Store,
    limit: Option<usize>,
) -> Result<Vec<HighlightListRow>> {
    let limit = limit
        .unwrap_or(DEFAULT_HIGHLIGHT_LIMIT)
        .clamp(1, MAX_HIGHLIGHT_LIMIT);
    store
        .list_highlights(limit)?
        .iter()
        .map(|record| HighlightListRow::from_record(store, record))
        .collect()
}

/// Create a note attached to a highlight.
#[tauri::command]
pub fn create_highlight_note(
    state: State<'_, AppState>,
    request: CreateHighlightNoteRequest,
) -> Result<NoteDetail> {
    let device = load_device_key(&state.paths)?;
    create_highlight_note_with_device(&state.store, &device, request)
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
}

/// Note DTO returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    /// Stable note id.
    pub id: String,
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

    Ok(NoteDetail::from_record(&record))
}

fn load_device_key(paths: &InstallPaths) -> Result<Keypair> {
    load_device_key_from_path(&paths.keys)
}

fn load_device_key_from_path(path: &Path) -> Result<Keypair> {
    Ok(Keystore::open(path).load_device()?)
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
    fn from_record(store: &carrel_store::Store, record: &HighlightWithItem) -> Result<Self> {
        let site_name = item_site_name(store, &record.highlight.item_id)?;
        Ok(Self {
            highlight: HighlightDetail::from_record(&record.highlight)?,
            item_title: record.item_title.clone(),
            item_source_name: source_name(record.item_primary_url.as_deref(), site_name.as_deref()),
            item_primary_url: record.item_primary_url.clone(),
            note_excerpt: latest_note_excerpt(store, &record.highlight.id)?,
        })
    }
}

impl NoteDetail {
    fn from_record(record: &NoteRecord) -> Self {
        Self {
            id: record.id.clone(),
            body: record.body.clone(),
            target_kind: record.target_kind.clone(),
            target_id: record.target_id.clone(),
            created_by: record.created_by.to_hex(),
            created_at: iso8601_timestamp(record.created_at_micros),
            created_at_micros: record.created_at_micros,
            updated_at_micros: record.updated_at_micros,
            signature: record.signature.to_hex(),
        }
    }
}

fn item_site_name(store: &carrel_store::Store, item_id: &str) -> Result<Option<String>> {
    let rows = store.query_with_params(
        r#"
        ?[site_name] :=
            *item_article{item_id, site_name},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    rows.rows
        .first()
        .map(|row| optional_string(required(row, 0, "item_article.site_name")?))
        .transpose()
        .map(Option::flatten)
}

fn latest_note_excerpt(store: &carrel_store::Store, highlight_id: &str) -> Result<Option<String>> {
    let notes = store.list_notes_for_target("highlight", highlight_id)?;
    Ok(notes.first().and_then(|note| note_excerpt(&note.body)))
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

fn required<'a>(
    row: &'a [DataValue],
    index: usize,
    context: &'static str,
) -> Result<&'a DataValue> {
    row.get(index).ok_or_else(|| AppError::InvalidData {
        context,
        value: "missing column".to_string(),
    })
}

fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Str(value) => Ok(Some(value.to_string())),
        other => Err(AppError::InvalidData {
            context: "optional string",
            value: format!("{other:?}"),
        }),
    }
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

        let rows = list_highlights_from_store(&store, Some(10)).unwrap();

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
    }

    #[test]
    fn loads_device_key_from_keystore_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let device = Keypair::from_seed(&[17_u8; 32]);
        Keystore::open(tempdir.path()).save_device(&device).unwrap();

        let loaded = load_device_key_from_path(tempdir.path()).unwrap();

        assert_eq!(loaded.public(), device.public());
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
