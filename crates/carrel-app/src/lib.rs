//! Tauri shell for the Carrel desktop application.

#![deny(unsafe_code)]

mod background;
mod close;
mod commands;
mod config;
mod error;
mod feed_runner;
mod state;
mod sync;

use std::str::FromStr;

use carrel_store::blobs::{BlobId, BlobStore};
use tauri::http::{Response, StatusCode, header};

pub use crate::error::{AppError, Result};

const BLOB_URI_SCHEME: &str = "carrel-blob";

/// Start the desktop application.
pub fn run() -> Result<()> {
    init_tracing();

    let paths = config::InstallPaths::resolve()?;
    let store = carrel_store::Store::open(&paths.store)?;
    let blobs = BlobStore::open(&paths.blobs);
    let protocol_blobs = blobs.clone();
    store.migrate()?;
    let app_state = state::AppState::new(store, blobs, paths);
    let background_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .manage(close::CloseState::default())
        .on_window_event(close::on_window_event)
        .register_uri_scheme_protocol(BLOB_URI_SCHEME, move |_ctx, request| {
            blob_protocol_response(&protocol_blobs, request.uri())
        })
        .setup(move |_| {
            background::spawn_feed_loop(background_state.clone());
            background::spawn_search_rebuild(background_state.clone());
            sync::spawn(background_state.clone());
            Ok(())
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            close::frontend_ready,
            close::finish_app_close,
            close::cancel_app_close,
            commands::links::open_external_url,
            commands::feeds::add_feed,
            commands::feeds::discover_feeds,
            commands::feeds::feed_status,
            commands::feeds::fetch_all_feeds,
            commands::feeds::fetch_feed,
            commands::feeds::list_feeds,
            commands::feeds::remove_feed,
            commands::subscriptions::import_subscriptions,
            commands::subscriptions::export_subscriptions,
            commands::highlights::create_highlight,
            commands::highlights::create_highlight_note,
            commands::highlights::edit_highlight_note,
            commands::highlights::resolve_highlight_note,
            commands::highlights::review_highlight_note,
            commands::highlights::remove_highlight,
            commands::annotation_export::export_annotations,
            commands::highlights::list_highlights,
            commands::highlights::list_item_highlights,
            commands::items::get_item,
            commands::items::list_items,
            commands::keymap::keymap_config,
            commands::items::mark_item_read,
            commands::items::mark_item_unread,
            commands::items::set_item_star,
            commands::items::update_read_progress,
            commands::status::version,
            commands::status::search_status,
            commands::status::rebuild_search,
            sync::sync_status,
            sync::enable_sync,
            sync::create_sync_invitation,
            sync::cancel_sync_invitation,
            sync::join_sync_invitation,
            sync::revoke_sync_device,
        ])
        .build(tauri::generate_context!())?
        .run(close::on_run_event);
    Ok(())
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "carrel_app=info,carrel_store=warn,tauri=warn".into());

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .without_time()
        .try_init();
}

fn blob_protocol_response(blobs: &BlobStore, uri: &tauri::http::Uri) -> Response<Vec<u8>> {
    let Some(raw_id) = blob_id_from_uri(uri) else {
        return response(
            StatusCode::BAD_REQUEST,
            "text/plain",
            b"missing blob id".to_vec(),
        );
    };

    let blob_id = match BlobId::from_str(&raw_id) {
        Ok(blob_id) => blob_id,
        Err(_) => {
            return response(
                StatusCode::BAD_REQUEST,
                "text/plain",
                b"invalid blob id".to_vec(),
            );
        }
    };

    match blobs.get_blocking(&blob_id) {
        Ok(bytes) => response(StatusCode::OK, infer_mime(bytes.as_ref()), bytes.to_vec()),
        Err(carrel_store::blobs::BlobError::NotFound { .. }) => response(
            StatusCode::NOT_FOUND,
            "text/plain",
            b"blob not found".to_vec(),
        ),
        Err(_) => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain",
            b"failed to read blob".to_vec(),
        ),
    }
}

fn blob_id_from_uri(uri: &tauri::http::Uri) -> Option<String> {
    let path = uri.path().trim_start_matches('/');
    if !path.is_empty() {
        return Some(path.to_string());
    }

    uri.host()
        .filter(|host| !host.eq_ignore_ascii_case("localhost"))
        .map(ToString::to_string)
}

fn response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .expect("static response builder should be valid")
}

fn infer_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if looks_like_svg(bytes) {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let trimmed = text.trim_start_matches(|char: char| char.is_whitespace() || char == '\u{feff}');
    trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_common_blob_mime_types() {
        assert_eq!(infer_mime(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(infer_mime(b"\xff\xd8\xffrest"), "image/jpeg");
        assert_eq!(infer_mime(b"RIFFxxxxWEBPrest"), "image/webp");
        assert_eq!(
            infer_mime(br#" <svg viewBox="0 0 1 1"></svg>"#),
            "image/svg+xml"
        );
        assert_eq!(infer_mime(b"plain"), "application/octet-stream");
    }
}
