//! Typed wrappers around Tauri commands.

use carrel_core::annotation::HighlightLocation;
use serde::{Deserialize, Serialize};

pub use carrel_core::annotation::{AnnotationCursor, AnnotationPage};
pub use carrel_core::library::{ItemCursor, ItemFilter, ItemPage, ItemView};

/// Operational state of locally derived cached-text search.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchStatus {
    #[default]
    Preparing,
    Ready,
    Partial,
    Failed,
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn search_status() -> Result<SearchStatus, ApiError> {
    let result = tauri_invoke("search_status", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;
    serde_wasm_bindgen::from_value(result).map_err(|error| ApiError::new(error.to_string()))
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn rebuild_search() -> Result<(), ApiError> {
    tauri_invoke("rebuild_search", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Choose an OPML file and import subscriptions. False means cancellation.
#[cfg(target_arch = "wasm32")]
pub async fn import_subscriptions() -> Result<bool, ApiError> {
    subscription_file_command("import_subscriptions").await
}

/// Choose an OPML destination. False means cancellation.
#[cfg(target_arch = "wasm32")]
pub async fn export_subscriptions() -> Result<bool, ApiError> {
    subscription_file_command("export_subscriptions").await
}

/// Choose a Markdown destination for all saved highlights and notes.
#[cfg(target_arch = "wasm32")]
pub async fn export_annotations() -> Result<bool, ApiError> {
    subscription_file_command("export_annotations").await
}

#[cfg(target_arch = "wasm32")]
async fn subscription_file_command(command: &str) -> Result<bool, ApiError> {
    let result = tauri_invoke(command, wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;
    serde_wasm_bindgen::from_value(result).map_err(|error| ApiError::new(error.to_string()))
}

/// Item row returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: String,
    pub title: String,
    pub source_name: String,
    pub length_label: String,
    pub time_label: String,
    pub read_state: String,
    pub starred: bool,
    pub primary_url: Option<String>,
    pub summary: Option<String>,
    pub discovered_at_micros: i64,
}

/// Full item payload returned by the native shell for the reading route.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub id: String,
    pub title: String,
    pub source_name: String,
    pub feed_url: Option<String>,
    pub published_at: Option<String>,
    pub language: Option<String>,
    pub length_label: String,
    pub estimated_read_minutes: Option<u32>,
    pub word_count: Option<u32>,
    pub time_label: String,
    pub discovered_at: String,
    pub read_state: String,
    pub starred: bool,
    pub primary_url: Option<String>,
    pub summary: Option<String>,
    pub content_html: String,
    pub creators: Vec<String>,
    pub byline: Option<String>,
    pub readable_blob_id: Option<String>,
    pub shape_tags: Vec<String>,
    pub last_scroll: Option<f64>,
    pub read_progress: Option<f64>,
    pub discovered_at_micros: i64,
}

/// Highlight creation request passed to the native shell.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHighlightRequest {
    pub item_id: String,
    pub location: HighlightLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Highlight note creation request passed to the native shell.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHighlightNoteRequest {
    pub highlight_id: String,
    pub body: String,
}

/// Save against the note revision the editor originally displayed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditHighlightNoteRequest {
    pub note_id: String,
    pub body: String,
    pub expected_revision_id: String,
}

/// Resolve only the complete set of versions actually reviewed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveHighlightNoteRequest {
    pub note_id: String,
    pub body: String,
    pub expected_revision_ids: Vec<String>,
}

/// Highlight payload returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightDetail {
    pub id: String,
    pub item_id: String,
    pub quoted_text: String,
    pub location: HighlightLocation,
    pub location_label: Option<String>,
    pub color: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub created_at_micros: i64,
    pub signature: String,
}

/// Library-level highlight row returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightListRow {
    pub highlight: HighlightDetail,
    pub item_title: String,
    pub item_source_name: String,
    pub item_primary_url: Option<String>,
    pub note_excerpt: Option<String>,
    pub notes: Vec<NoteDetail>,
    pub can_remove: bool,
}

/// Note payload returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: String,
    pub revision_id: String,
    pub superseded_revision_ids: Vec<String>,
    pub conflicts: Vec<NoteConflictVersion>,
    pub body: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
    pub signature: String,
    pub can_edit: bool,
}

/// A complete concurrent draft, never an excerpt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteConflictVersion {
    pub revision_id: String,
    pub body: String,
    pub saved_at: String,
    pub device_label: String,
}

/// Item-state mutation request.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStateRequest {
    pub item_id: String,
}

/// Reading-progress mutation request.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadProgressUpdate {
    pub item_id: String,
    pub progress: f64,
    pub scroll_y: f64,
}

/// User keymap configuration returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapConfig {
    pub path: String,
    pub overrides: Vec<KeymapBindingOverride>,
    pub warnings: Vec<String>,
}

/// One valid user keymap override.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapBindingOverride {
    pub layer: String,
    pub key: String,
    pub action: String,
}

/// Feed subscription returned by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSummary {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub display_name: String,
    pub last_fetched_label: String,
    pub fetch_interval_seconds: i64,
    pub consecutive_failures: i64,
    pub folder: Option<String>,
    pub auto_mark_read: bool,
}

/// Summary of one native feed fetch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedFetchSummary {
    pub url: String,
    pub outcome: String,
    pub message: String,
    pub new_items: usize,
    pub updated_items: usize,
    pub entry_errors: usize,
    pub content_errors: usize,
    pub feed_errors: usize,
}

/// Current feed activity status for the status strip.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedStatusSummary {
    pub is_fetching: bool,
    pub message: String,
    pub new_items: usize,
    pub updated_items: usize,
    pub entry_errors: usize,
    pub content_errors: usize,
    pub feed_errors: usize,
    pub revision: u64,
}

impl Default for FeedStatusSummary {
    fn default() -> Self {
        Self {
            is_fetching: false,
            message: "Local store ready".to_string(),
            new_items: 0,
            updated_items: 0,
            entry_errors: 0,
            content_errors: 0,
            feed_errors: 0,
            revision: 0,
        }
    }
}

/// Error returned while invoking a native command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiError {
    message: String,
    kind: Option<String>,
}

impl ApiError {
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_annotation_conflict(&self) -> bool {
        self.kind.as_deref() == Some("annotation_conflict")
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: None,
        }
    }
}

/// Load items for a list route.
#[cfg(target_arch = "wasm32")]
pub async fn list_items(filter: ItemFilter) -> Result<ItemPage<ItemSummary>, ApiError> {
    #[derive(Serialize)]
    struct Args {
        filter: ItemFilter,
    }

    let args = serde_wasm_bindgen::to_value(&Args { filter })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let rows = tauri_invoke("list_items", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(rows).map_err(|error| ApiError::new(error.to_string()))
}

/// Load items for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn list_items(_filter: ItemFilter) -> Result<ItemPage<ItemSummary>, ApiError> {
    Ok(ItemPage {
        items: Vec::new(),
        next_cursor: None,
    })
}

/// Load one item for the reading route.
#[cfg(target_arch = "wasm32")]
pub async fn get_item(id: String) -> Result<Option<ItemDetail>, ApiError> {
    #[derive(Serialize)]
    struct Args {
        id: String,
    }

    let args = serde_wasm_bindgen::to_value(&Args { id })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let row = tauri_invoke("get_item", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(row).map_err(|error| ApiError::new(error.to_string()))
}

/// Load one item for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn get_item(_id: String) -> Result<Option<ItemDetail>, ApiError> {
    Ok(None)
}

/// Persist local reading progress.
#[cfg(target_arch = "wasm32")]
pub async fn update_read_progress(update: ReadProgressUpdate) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct Args {
        update: ReadProgressUpdate,
    }

    let args = serde_wasm_bindgen::to_value(&Args { update })
        .map_err(|error| ApiError::new(error.to_string()))?;
    tauri_invoke("update_read_progress", args)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Persist local reading progress for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn update_read_progress(_update: ReadProgressUpdate) -> Result<(), ApiError> {
    Ok(())
}

/// Mark one item read.
#[cfg(target_arch = "wasm32")]
pub async fn mark_item_read(request: ItemStateRequest) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: ItemStateRequest,
    }

    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    tauri_invoke("mark_item_read", args)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Mark one item read for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn mark_item_read(_request: ItemStateRequest) -> Result<(), ApiError> {
    Ok(())
}

/// Mark one item unread.
#[cfg(target_arch = "wasm32")]
pub async fn mark_item_unread(request: ItemStateRequest) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: ItemStateRequest,
    }

    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    tauri_invoke("mark_item_unread", args)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Mark one item unread for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn mark_item_unread(_request: ItemStateRequest) -> Result<(), ApiError> {
    Ok(())
}

/// Toggle the local starred state and return the new value.
#[cfg(target_arch = "wasm32")]
pub async fn toggle_item_star(request: ItemStateRequest) -> Result<bool, ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: ItemStateRequest,
    }

    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("toggle_item_star", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Toggle the local starred state for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn toggle_item_star(_request: ItemStateRequest) -> Result<bool, ApiError> {
    Ok(false)
}

/// Create a highlight.
#[cfg(target_arch = "wasm32")]
pub async fn create_highlight(
    request: CreateHighlightRequest,
) -> Result<HighlightDetail, ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: CreateHighlightRequest,
    }

    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("create_highlight", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Create a highlight for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn create_highlight(
    request: CreateHighlightRequest,
) -> Result<HighlightDetail, ApiError> {
    Ok(HighlightDetail {
        id: String::new(),
        item_id: request.item_id,
        quoted_text: String::new(),
        location: request.location,
        location_label: request.location_label,
        color: request.color,
        created_by: String::new(),
        created_at: String::new(),
        created_at_micros: 0,
        signature: String::new(),
    })
}

/// List highlights for one item.
#[cfg(target_arch = "wasm32")]
pub async fn list_item_highlights(item_id: String) -> Result<Vec<HighlightDetail>, ApiError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        item_id: String,
    }

    let args = serde_wasm_bindgen::to_value(&Args { item_id })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let rows = tauri_invoke("list_item_highlights", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(rows).map_err(|error| ApiError::new(error.to_string()))
}

/// List highlights for one item for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn list_item_highlights(_item_id: String) -> Result<Vec<HighlightDetail>, ApiError> {
    Ok(Vec::new())
}

/// List highlights across the local library.
#[cfg(target_arch = "wasm32")]
pub async fn list_highlights(
    limit: Option<usize>,
    after: Option<AnnotationCursor>,
) -> Result<AnnotationPage<HighlightListRow>, ApiError> {
    #[derive(Serialize)]
    struct Args {
        limit: Option<usize>,
        after: Option<AnnotationCursor>,
    }

    let args = serde_wasm_bindgen::to_value(&Args { limit, after })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let rows = tauri_invoke("list_highlights", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(rows).map_err(|error| ApiError::new(error.to_string()))
}

/// List highlights across the local library for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn list_highlights(
    _limit: Option<usize>,
    _after: Option<AnnotationCursor>,
) -> Result<AnnotationPage<HighlightListRow>, ApiError> {
    Ok(AnnotationPage {
        items: Vec::new(),
        next_cursor: None,
    })
}

/// Update a note without replacing a newer revision silently.
#[cfg(target_arch = "wasm32")]
pub async fn edit_highlight_note(
    request: EditHighlightNoteRequest,
) -> Result<NoteDetail, ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: EditHighlightNoteRequest,
    }
    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("edit_highlight_note", args)
        .await
        .map_err(api_error_from_js)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Read every current version directly for stale-editor recovery.
#[cfg(target_arch = "wasm32")]
pub async fn review_highlight_note(note_id: String) -> Result<NoteDetail, ApiError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        note_id: String,
    }
    let args = serde_wasm_bindgen::to_value(&Args { note_id })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("review_highlight_note", args)
        .await
        .map_err(api_error_from_js)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Save an explicit resolution against every version shown in the editor.
#[cfg(target_arch = "wasm32")]
pub async fn resolve_highlight_note(
    request: ResolveHighlightNoteRequest,
) -> Result<NoteDetail, ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: ResolveHighlightNoteRequest,
    }
    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("resolve_highlight_note", args)
        .await
        .map_err(api_error_from_js)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Remove a highlight through the signed local store boundary.
#[cfg(target_arch = "wasm32")]
pub async fn remove_highlight(highlight_id: String) -> Result<(), ApiError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        highlight_id: String,
    }
    let args = serde_wasm_bindgen::to_value(&Args { highlight_id })
        .map_err(|error| ApiError::new(error.to_string()))?;
    tauri_invoke("remove_highlight", args)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Create a note attached to a highlight.
#[cfg(target_arch = "wasm32")]
pub async fn create_highlight_note(
    request: CreateHighlightNoteRequest,
) -> Result<NoteDetail, ApiError> {
    #[derive(Serialize)]
    struct Args {
        request: CreateHighlightNoteRequest,
    }

    let args = serde_wasm_bindgen::to_value(&Args { request })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let value = tauri_invoke("create_highlight_note", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Create a highlight note for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn create_highlight_note(
    request: CreateHighlightNoteRequest,
) -> Result<NoteDetail, ApiError> {
    Ok(NoteDetail {
        id: String::new(),
        revision_id: String::new(),
        superseded_revision_ids: Vec::new(),
        conflicts: Vec::new(),
        body: request.body,
        target_kind: Some("highlight".to_string()),
        target_id: Some(request.highlight_id),
        created_by: String::new(),
        created_at: String::new(),
        created_at_micros: 0,
        updated_at_micros: 0,
        signature: String::new(),
        can_edit: true,
    })
}

/// Load keymap overrides for the webview.
#[cfg(target_arch = "wasm32")]
pub async fn keymap_config() -> Result<KeymapConfig, ApiError> {
    let value = tauri_invoke("keymap_config", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(value).map_err(|error| ApiError::new(error.to_string()))
}

/// Load keymap overrides for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn keymap_config() -> Result<KeymapConfig, ApiError> {
    Ok(KeymapConfig {
        path: String::new(),
        overrides: Vec::new(),
        warnings: Vec::new(),
    })
}

/// Load feed subscriptions.
#[cfg(target_arch = "wasm32")]
pub async fn list_feeds() -> Result<Vec<FeedSummary>, ApiError> {
    let rows = tauri_invoke("list_feeds", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(rows).map_err(|error| ApiError::new(error.to_string()))
}

/// Load feed subscriptions for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn list_feeds() -> Result<Vec<FeedSummary>, ApiError> {
    Ok(Vec::new())
}

/// Add a feed subscription.
#[cfg(target_arch = "wasm32")]
pub async fn add_feed(url: String) -> Result<FeedSummary, ApiError> {
    #[derive(Serialize)]
    struct Args {
        url: String,
    }

    let args = serde_wasm_bindgen::to_value(&Args { url })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let feed = tauri_invoke("add_feed", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(feed).map_err(|error| ApiError::new(error.to_string()))
}

/// Add a feed subscription for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn add_feed(_url: String) -> Result<FeedSummary, ApiError> {
    Ok(FeedSummary {
        url: String::new(),
        title: None,
        description: None,
        display_name: String::new(),
        last_fetched_label: "never".to_string(),
        fetch_interval_seconds: 0,
        consecutive_failures: 0,
        folder: None,
        auto_mark_read: false,
    })
}

/// Remove a feed subscription without deleting saved items.
#[cfg(target_arch = "wasm32")]
pub async fn remove_feed(url: String) -> Result<bool, ApiError> {
    #[derive(Serialize)]
    struct Args {
        url: String,
    }

    let args = serde_wasm_bindgen::to_value(&Args { url })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let removed = tauri_invoke("remove_feed", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(removed).map_err(|error| ApiError::new(error.to_string()))
}

/// Remove a feed subscription for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn remove_feed(_url: String) -> Result<bool, ApiError> {
    Ok(false)
}

/// Fetch one feed now.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_feed(url: String) -> Result<Option<FeedFetchSummary>, ApiError> {
    #[derive(Serialize)]
    struct Args {
        url: String,
    }

    let args = serde_wasm_bindgen::to_value(&Args { url })
        .map_err(|error| ApiError::new(error.to_string()))?;
    let summary = tauri_invoke("fetch_feed", args)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(summary).map_err(|error| ApiError::new(error.to_string()))
}

/// Fetch one feed for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_feed(_url: String) -> Result<Option<FeedFetchSummary>, ApiError> {
    Ok(None)
}

/// Fetch every subscribed feed now.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_all_feeds() -> Result<Vec<FeedFetchSummary>, ApiError> {
    let summaries = tauri_invoke("fetch_all_feeds", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(summaries).map_err(|error| ApiError::new(error.to_string()))
}

/// Fetch every subscribed feed for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_all_feeds() -> Result<Vec<FeedFetchSummary>, ApiError> {
    Ok(Vec::new())
}

/// Load current feed activity status.
#[cfg(target_arch = "wasm32")]
pub async fn feed_status() -> Result<FeedStatusSummary, ApiError> {
    let status = tauri_invoke("feed_status", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map_err(api_error_from_js)?;

    serde_wasm_bindgen::from_value(status).map_err(|error| ApiError::new(error.to_string()))
}

/// Load current feed activity status for SSR tests and non-wasm builds.
#[cfg(not(target_arch = "wasm32"))]
pub async fn feed_status() -> Result<FeedStatusSummary, ApiError> {
    Ok(FeedStatusSummary::default())
}

/// Inform the native close coordinator that the frontend listener is mounted.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn frontend_ready() -> Result<(), ApiError> {
    tauri_invoke("frontend_ready", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Acknowledge that all local saves have completed before normal exit.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn finish_app_close() -> Result<(), ApiError> {
    tauri_invoke("finish_app_close", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Leave the window open so a failed close-time save can be retried.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn cancel_app_close() -> Result<(), ApiError> {
    tauri_invoke("cancel_app_close", wasm_bindgen::JsValue::UNDEFINED)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

/// Open an article link in the system browser without replacing the reader.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn open_external_url(url: String) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct Args {
        url: String,
    }
    let args = serde_wasm_bindgen::to_value(&Args { url })
        .map_err(|error| ApiError::new(error.to_string()))?;
    tauri_invoke("open_external_url", args)
        .await
        .map(|_| ())
        .map_err(api_error_from_js)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(
        js_namespace = ["window", "__TAURI__", "core"],
        js_name = invoke,
        catch
    )]
    async fn tauri_invoke(
        command: &str,
        args: wasm_bindgen::JsValue,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
}

#[cfg(target_arch = "wasm32")]
fn api_error_from_js(value: wasm_bindgen::JsValue) -> ApiError {
    if let Some(message) = value.as_string() {
        return ApiError::new(message);
    }

    match serde_wasm_bindgen::from_value::<CommandError>(value) {
        Ok(error) => ApiError {
            message: error.message,
            kind: error.kind,
        },
        Err(error) => ApiError::new(error.to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
struct CommandError {
    message: String,
    kind: Option<String>,
}
