//! Shared state managed by Tauri.

use std::sync::{Arc, Mutex, atomic::AtomicBool};

use carrel_store::Store;
use carrel_store::blobs::BlobStore;
use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

use crate::config::InstallPaths;

/// State available to command handlers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) blobs: BlobStore,
    pub(crate) feed_fetch_lock: Arc<AsyncMutex<()>>,
    // Read/modify/write commands must not race a delayed progress snapshot.
    pub(crate) item_state_lock: Arc<Mutex<()>>,
    // Includes first-use key creation, before the store's annotation write lock.
    pub(crate) annotation_lock: Arc<Mutex<()>>,
    pub(crate) feed_status: FeedStatusHandle,
    pub(crate) search_status: Arc<Mutex<SearchStatus>>,
    pub(crate) search_rebuilding: Arc<AtomicBool>,
    pub(crate) store: Store,
    pub(crate) paths: InstallPaths,
}

impl AppState {
    pub(crate) fn new(store: Store, blobs: BlobStore, paths: InstallPaths) -> Self {
        Self {
            blobs,
            feed_fetch_lock: Arc::new(AsyncMutex::new(())),
            item_state_lock: Arc::new(Mutex::new(())),
            annotation_lock: Arc::new(Mutex::new(())),
            feed_status: FeedStatusHandle::new(),
            search_status: Arc::new(Mutex::new(SearchStatus::Preparing)),
            search_rebuilding: Arc::new(AtomicBool::new(false)),
            store,
            paths,
        }
    }
}

/// Operational state of the local cached-text rebuild, never a synced fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchStatus {
    Preparing,
    Ready,
    Partial,
    Failed,
}

/// Snapshot of quiet feed activity for the status strip.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeedStatusSnapshot {
    /// Whether a manual or background feed fetch is currently running.
    pub is_fetching: bool,
    /// Short status-strip message.
    pub message: String,
    /// Newly discovered items from the last completed fetch run.
    pub new_items: usize,
    /// Existing items updated from the last completed fetch run.
    pub updated_items: usize,
    /// Entry ingest errors from the last completed fetch run.
    pub entry_errors: usize,
    /// Readable-content extraction/cache errors from the last completed fetch run.
    pub content_errors: usize,
    /// Whole-feed failures from the last completed fetch run.
    pub feed_errors: usize,
    /// Monotonic revision incremented whenever status changes.
    pub revision: u64,
}

impl Default for FeedStatusSnapshot {
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

/// Aggregated counts from a fetch run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FeedStatusCounts {
    pub(crate) new_items: usize,
    pub(crate) updated_items: usize,
    pub(crate) entry_errors: usize,
    pub(crate) content_errors: usize,
    pub(crate) feed_errors: usize,
}

/// Shared feed status updated by commands and background workers.
#[derive(Clone, Debug)]
pub(crate) struct FeedStatusHandle {
    inner: Arc<Mutex<FeedStatusSnapshot>>,
}

impl FeedStatusHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FeedStatusSnapshot::default())),
        }
    }

    pub(crate) fn snapshot(&self) -> FeedStatusSnapshot {
        self.inner
            .lock()
            .expect("feed status lock should not be poisoned")
            .clone()
    }

    pub(crate) fn set_fetching(&self, message: impl Into<String>) -> FeedStatusSnapshot {
        self.update(|snapshot| {
            snapshot.is_fetching = true;
            snapshot.message = message.into();
        })
    }

    pub(crate) fn set_idle(
        &self,
        message: impl Into<String>,
        counts: FeedStatusCounts,
    ) -> FeedStatusSnapshot {
        self.update(|snapshot| {
            snapshot.is_fetching = false;
            snapshot.message = message.into();
            snapshot.new_items = counts.new_items;
            snapshot.updated_items = counts.updated_items;
            snapshot.entry_errors = counts.entry_errors;
            snapshot.content_errors = counts.content_errors;
            snapshot.feed_errors = counts.feed_errors;
        })
    }

    fn update(&self, apply: impl FnOnce(&mut FeedStatusSnapshot)) -> FeedStatusSnapshot {
        let mut snapshot = self
            .inner
            .lock()
            .expect("feed status lock should not be poisoned");
        apply(&mut snapshot);
        snapshot.revision = snapshot.revision.saturating_add(1);
        snapshot.clone()
    }
}
