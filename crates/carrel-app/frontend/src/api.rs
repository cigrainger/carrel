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
