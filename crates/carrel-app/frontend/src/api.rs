//! Typed wrappers around Tauri commands.

use serde::{Deserialize, Serialize};

/// Item-list filter passed to the app shell.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub view: ItemView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl ItemFilter {
    pub fn today() -> Self {
        Self {
            view: ItemView::Today,
            limit: Some(100),
        }
    }
}

/// Supported item-list views.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemView {
    Today,
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
    pub published_at: Option<String>,
    pub language: Option<String>,
    pub length_label: String,
    pub estimated_read_minutes: Option<u32>,
    pub time_label: String,
    pub read_state: String,
    pub starred: bool,
    pub primary_url: Option<String>,
    pub summary: Option<String>,
    pub content_html: String,
    pub creators: Vec<String>,
    pub byline: Option<String>,
    pub readable_blob_id: Option<String>,
    pub last_scroll: Option<f64>,
    pub discovered_at_micros: i64,
}

/// Item-state mutation request.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStateRequest {
    pub item_id: String,
}

/// Reading-progress mutation request.
#[derive(Clone, Debug, Serialize)]
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
}

impl ApiError {
    pub fn message(&self) -> &str {
        &self.message
    }

    #[cfg(target_arch = "wasm32")]
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Load items for a list route.
#[cfg(target_arch = "wasm32")]
pub async fn list_items(filter: ItemFilter) -> Result<Vec<ItemSummary>, ApiError> {
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
pub async fn list_items(_filter: ItemFilter) -> Result<Vec<ItemSummary>, ApiError> {
    Ok(Vec::new())
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
        Ok(error) => ApiError::new(error.message),
        Err(error) => ApiError::new(error.to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
struct CommandError {
    message: String,
}
