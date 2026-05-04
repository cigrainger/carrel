//! Shared state managed by Tauri.

use carrel_store::Store;

use crate::config::InstallPaths;

/// State available to command handlers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) store: Store,
    pub(crate) paths: InstallPaths,
}

impl AppState {
    pub(crate) fn new(store: Store, paths: InstallPaths) -> Self {
        Self { store, paths }
    }
}
