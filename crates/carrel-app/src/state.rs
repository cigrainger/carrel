//! Shared state managed by Tauri.

use carrel_store::Store;
use carrel_store::blobs::BlobStore;

use crate::config::InstallPaths;

/// State available to command handlers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) blobs: BlobStore,
    pub(crate) store: Store,
    pub(crate) paths: InstallPaths,
}

impl AppState {
    pub(crate) fn new(store: Store, blobs: BlobStore, paths: InstallPaths) -> Self {
        Self {
            blobs,
            store,
            paths,
        }
    }
}
