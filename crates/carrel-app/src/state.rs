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
    pub(crate) backup: Arc<Mutex<crate::backup::BackupReview>>,
    pub(crate) blobs: BlobStore,
    pub(crate) feed_fetch_lock: Arc<AsyncMutex<()>>,
    // Includes first-use key creation, before the store's annotation write lock.
    pub(crate) annotation_lock: Arc<Mutex<()>>,
    pub(crate) feed_status: FeedStatusHandle,
    pub(crate) search_status: Arc<Mutex<SearchStatus>>,
    pub(crate) search_rebuilding: Arc<AtomicBool>,
    pub(crate) store: Store,
    pub(crate) paths: InstallPaths,
    pub(crate) sync: Arc<AsyncMutex<crate::sync::SyncState>>,
}

impl AppState {
    pub(crate) fn new(store: Store, blobs: BlobStore, paths: InstallPaths) -> Self {
        Self {
            backup: Arc::new(Mutex::new(crate::backup::BackupReview::default())),
            blobs,
            feed_fetch_lock: Arc::new(AsyncMutex::new(())),
            annotation_lock: Arc::new(Mutex::new(())),
            feed_status: FeedStatusHandle::new(),
            search_status: Arc::new(Mutex::new(SearchStatus::Preparing)),
            search_rebuilding: Arc::new(AtomicBool::new(false)),
            store,
            paths,
            sync: Arc::new(AsyncMutex::new(crate::sync::SyncState::default())),
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
    /// Latest unresolved source problems, retained across unrelated fetches.
    pub issues: Vec<FeedIssue>,
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
            issues: Vec::new(),
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

/// A source-scoped recovery message; diagnostic detail is deliberately opened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeedIssue {
    pub feed_url: String,
    pub kind: String,
    pub message: String,
    pub detail: String,
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

    pub(crate) fn finish_reports(
        &self,
        message: String,
        counts: FeedStatusCounts,
        reports: &[crate::feed_runner::FetchReport],
    ) {
        self.update(|snapshot| {
            snapshot.is_fetching = false;
            snapshot.message = message;
            snapshot.new_items = counts.new_items;
            snapshot.updated_items = counts.updated_items;
            snapshot.entry_errors = counts.entry_errors;
            snapshot.content_errors = counts.content_errors;
            snapshot.feed_errors = counts.feed_errors;
            for report in reports {
                // A 304 proves the feed is reachable, not that a previously
                // failed article cache has recovered.
                snapshot.issues.retain(|issue| {
                    issue.feed_url != report.url
                        || (!matches!(
                            report.outcome,
                            crate::feed_runner::FetchOutcome::Updated { .. }
                        ) && issue.kind != "feed")
                });
                snapshot
                    .issues
                    .extend(crate::commands::feeds::report_issues(report));
            }
        });
    }

    pub(crate) fn forget_source(&self, url: &str) {
        self.update(|snapshot| snapshot.issues.retain(|issue| issue.feed_url != url));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed_runner::{FetchOutcome, FetchReport};

    fn report(url: &str, outcome: FetchOutcome) -> FetchReport {
        FetchReport {
            url: url.into(),
            outcome,
            content_errors: Vec::new(),
        }
    }

    #[test]
    fn unrelated_checks_and_304_do_not_hide_failed_article_caches() {
        let status = FeedStatusHandle::new();
        let mut partial = report(
            "a",
            FetchOutcome::Updated {
                stats: Default::default(),
            },
        );
        partial
            .content_errors
            .push(carrel_store::feeds::EntryError {
                feed_guid: "one".into(),
                message: "cache failed".into(),
            });
        status.finish_reports("Updated".into(), Default::default(), &[partial]);
        status.finish_reports(
            "Checked".into(),
            Default::default(),
            &[report("b", FetchOutcome::NotModified)],
        );
        status.finish_reports(
            "Checked".into(),
            Default::default(),
            &[report("a", FetchOutcome::NotModified)],
        );
        assert_eq!(status.snapshot().issues.len(), 1);
        status.set_fetching("Retrying");
        assert_eq!(status.snapshot().issues.len(), 1);
        status.finish_reports(
            "Offline".into(),
            Default::default(),
            &[report("a", FetchOutcome::HttpStatus { status: 503 })],
        );
        assert_eq!(status.snapshot().issues.len(), 2);
        status.finish_reports(
            "Checked".into(),
            Default::default(),
            &[report("a", FetchOutcome::NotModified)],
        );
        assert_eq!(status.snapshot().issues.len(), 1);
        status.finish_reports(
            "Updated".into(),
            Default::default(),
            &[report(
                "a",
                FetchOutcome::Updated {
                    stats: Default::default(),
                },
            )],
        );
        assert!(status.snapshot().issues.is_empty());
    }

    #[test]
    fn recovered_and_removed_sources_drop_only_their_own_problems() {
        let status = FeedStatusHandle::new();
        status.finish_reports(
            "Failed".into(),
            Default::default(),
            &[
                report("a", FetchOutcome::HttpStatus { status: 500 }),
                report(
                    "b",
                    FetchOutcome::Error {
                        message: "offline".into(),
                    },
                ),
            ],
        );
        status.finish_reports(
            "Checked".into(),
            Default::default(),
            &[report("a", FetchOutcome::NotModified)],
        );
        assert_eq!(status.snapshot().issues[0].feed_url, "b");
        status.forget_source("b");
        assert!(status.snapshot().issues.is_empty());
    }
}
