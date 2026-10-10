//! Native file boundaries for portable subscription interchange.

use std::io::{Read, Write};
use std::path::Path;

use carrel_feeds::opml::{MAX_OPML_BYTES, export_opml, parse_opml};
use carrel_store::Store;
use tauri::{State, Window};
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, Result};
use crate::state::{AppState, FeedStatusCounts};

/// Choose and atomically import an OPML file; false means cancellation.
#[tauri::command]
pub async fn import_subscriptions(window: Window, state: State<'_, AppState>) -> Result<bool> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import subscriptions")
        .add_filter("OPML subscriptions", &["opml", "xml"])
        .pick_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver.await.map_err(file_error)? else {
        return Ok(false);
    };
    let path = file.into_path().map_err(file_error)?;
    let store = state.store.clone();
    let device = super::highlights::load_device_key(&state.paths)?;
    tauri::async_runtime::spawn_blocking(move || {
        import_from_path_with_signer(&store, &path, &device)
    })
    .await
    .map_err(file_error)??;
    state.feed_status.set_idle(
        "Subscriptions imported; existing settings kept",
        FeedStatusCounts::default(),
    );
    Ok(true)
}

/// Choose a destination and export subscriptions; false means cancellation.
#[tauri::command]
pub async fn export_subscriptions(window: Window, state: State<'_, AppState>) -> Result<bool> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Export subscriptions")
        .set_file_name("carrel-subscriptions.opml")
        .add_filter("OPML subscriptions", &["opml"])
        .save_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver.await.map_err(file_error)? else {
        return Ok(false);
    };
    let path = file.into_path().map_err(file_error)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || export_to_path(&store, &path))
        .await
        .map_err(file_error)??;
    state
        .feed_status
        .set_idle("Subscriptions exported", FeedStatusCounts::default());
    Ok(true)
}

#[cfg(test)]
fn import_from_path(store: &Store, path: &Path) -> Result<()> {
    import_from_path_with_signer(
        store,
        path,
        &carrel_core::identity::Keypair::from_seed(&[22; 32]),
    )
}

fn import_from_path_with_signer(
    store: &Store,
    path: &Path,
    signer: &carrel_core::identity::Keypair,
) -> Result<()> {
    let file = std::fs::File::open(path).map_err(file_error)?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_OPML_BYTES + 1).expect("fixed byte limit"))
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    let subscriptions = parse_opml(&bytes).map_err(file_error)?;
    if subscriptions.is_empty() {
        return Err(AppError::SubscriptionFile(
            "This OPML file contains no feed subscriptions".into(),
        ));
    }
    store.import_private_subscriptions(&subscriptions, crate::feed_runner::now_micros(), signer)?;
    Ok(())
}

fn export_to_path(store: &Store, path: &Path) -> Result<()> {
    let bytes = export_opml(&store.feed_subscriptions()?).map_err(file_error)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::SubscriptionFile("The destination has no parent folder".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(file_error)?;
    temporary.write_all(&bytes).map_err(file_error)?;
    temporary.as_file().sync_all().map_err(file_error)?;
    temporary.persist(path).map_err(file_error)?;
    Ok(())
}

fn file_error(error: impl std::fmt::Display) -> AppError {
    AppError::SubscriptionFile(format!("Subscription file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_files_export_and_import_and_repeated_import_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("subscriptions.opml");
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.add_feed("https://example.com/feed").unwrap();
        std::fs::write(&path, "old export").unwrap();
        export_to_path(&store, &path).unwrap();
        let other = Store::open_in_memory().unwrap();
        other.migrate().unwrap();
        import_from_path(&other, &path).unwrap();
        assert_eq!(other.list_feeds().unwrap().len(), 1);
        import_from_path(&other, &path).unwrap();
        assert_eq!(other.list_feeds().unwrap().len(), 1);
    }

    #[test]
    fn invalid_missing_and_oversized_files_leave_existing_subscriptions_alone() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("subscriptions.opml");
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.add_feed("https://example.com/feed").unwrap();
        let original = store.list_feeds().unwrap();
        assert!(import_from_path(&store, &path).is_err());
        for bytes in [b"<opml><body/></opml>".to_vec(), b"<opml><body><outline xmlUrl='https://valid.example/feed'/><outline xmlUrl='file:///private/data'/></body></opml>".to_vec(), vec![b' '; MAX_OPML_BYTES + 1]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(import_from_path(&store, &path).is_err());
            assert_eq!(store.list_feeds().unwrap(), original);
        }
        assert!(export_to_path(&store, &directory.path().join("missing/file.opml")).is_err());
    }
}
