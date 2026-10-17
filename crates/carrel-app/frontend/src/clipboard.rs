//! Start clipboard writes during user activation, before the first await.

#[cfg(target_arch = "wasm32")]
pub(crate) fn start_copy(text: &str) -> Option<web_sys::js_sys::Promise> {
    let clipboard = web_sys::window()?.navigator().clipboard();
    let value: &wasm_bindgen::JsValue = clipboard.as_ref();
    value.is_object().then(|| clipboard.write_text(text))
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn finish_copy(copy: Option<web_sys::js_sys::Promise>) -> bool {
    match copy {
        Some(copy) => wasm_bindgen_futures::JsFuture::from(copy).await.is_ok(),
        None => false,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn start_copy(_text: &str) -> Option<()> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn finish_copy(_copy: Option<()>) -> bool {
    false
}
