//! Coalesce scroll snapshots and serialize progress writes across route changes.

use crate::api::{self, ReadProgressUpdate};
use crate::feeds::{FeedController, use_feed_controller};
use crate::status::StatusController;
use crate::writes::{PendingWrites, use_pending_writes};
use leptos::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct ProgressQueue {
    pending: BTreeMap<String, ReadProgressUpdate>,
    latest: BTreeMap<String, ReadProgressUpdate>,
    running: bool,
    timer_pending: bool,
}

impl ProgressQueue {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn record(&mut self, update: ReadProgressUpdate) -> bool {
        self.latest.insert(update.item_id.clone(), update.clone());
        self.pending.insert(update.item_id.clone(), update);
        if self.timer_pending {
            return false;
        }
        self.timer_pending = true;
        true
    }

    fn start(&mut self) -> bool {
        self.timer_pending = false;
        if self.running || self.pending.is_empty() {
            return false;
        }
        self.running = true;
        true
    }

    fn next(&mut self) -> Option<ReadProgressUpdate> {
        let update = self.pending.pop_first().map(|(_, update)| update);
        if update.is_none() {
            self.running = false;
        }
        update
    }

    fn confirmed(&mut self, update: &ReadProgressUpdate) {
        if self.latest.get(&update.item_id) == Some(update) {
            self.latest.remove(&update.item_id);
        }
    }

    fn retry_latest(&mut self) {
        self.pending.extend(self.latest.clone());
    }
}

#[derive(Clone)]
pub(crate) struct ProgressWriter {
    queue: Arc<Mutex<ProgressQueue>>,
    status: StatusController,
    feeds: FeedController,
    writes: PendingWrites,
}

impl ProgressWriter {
    pub(crate) fn new(status: StatusController, feeds: FeedController) -> Self {
        Self {
            queue: Arc::default(),
            status,
            feeds,
            writes: use_pending_writes(),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn latest(&self, item_id: &str) -> Option<ReadProgressUpdate> {
        self.queue
            .lock()
            .expect("progress queue poisoned")
            .latest
            .get(item_id)
            .cloned()
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn record(&self, update: ReadProgressUpdate) {
        let schedule = self
            .queue
            .lock()
            .expect("progress queue poisoned")
            .record(update);
        if schedule {
            #[cfg(target_arch = "wasm32")]
            {
                let writer = self.clone();
                leptos::prelude::set_timeout(
                    move || writer.flush(),
                    std::time::Duration::from_millis(500),
                );
            }
        }
    }

    pub(crate) fn flush(&self) {
        if !self.queue.lock().expect("progress queue poisoned").start() {
            return;
        }
        let writer = self.clone();
        let guard = self.writes.begin();
        spawn_progress_task(async move {
            let _guard = guard;
            loop {
                let update = writer.queue.lock().expect("progress queue poisoned").next();
                let Some(update) = update else {
                    break;
                };
                if api::update_read_progress(update.clone()).await.is_err() {
                    writer.status.warn("Could not save your reading position.");
                } else {
                    writer
                        .queue
                        .lock()
                        .expect("progress queue poisoned")
                        .confirmed(&update);
                    writer.feeds.bump_refresh();
                }
            }
        });
    }

    /// Retry earlier failed snapshots as well as the latest coalesced positions.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn flush_for_close(&self) {
        let mut queue = self.queue.lock().expect("progress queue poisoned");
        queue.retry_latest();
        drop(queue);
        self.flush();
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn has_unsaved_positions(&self) -> bool {
        !self
            .queue
            .lock()
            .expect("progress queue poisoned")
            .latest
            .is_empty()
    }
}

pub(crate) fn provide_progress_writer() {
    provide_context(ProgressWriter::new(
        crate::status::use_status_controller(),
        use_feed_controller(),
    ));
}

pub(crate) fn use_progress_writer() -> ProgressWriter {
    use_context::<ProgressWriter>().expect("ProgressWriter should be mounted")
}

#[cfg(target_arch = "wasm32")]
fn spawn_progress_task(task: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn_progress_task(_task: impl std::future::Future<Output = ()> + 'static) {}

#[cfg(test)]
mod tests {
    use super::*;
    fn update(id: &str, y: f64) -> ReadProgressUpdate {
        ReadProgressUpdate {
            item_id: id.into(),
            progress: 0.5,
            scroll_y: y,
        }
    }

    #[test]
    fn a_scroll_burst_keeps_only_its_last_position() {
        let mut queue = ProgressQueue::default();
        assert!(queue.record(update("a", 10.0)));
        assert!(!queue.record(update("a", 20.0)));
        assert!(queue.start());
        assert_eq!(queue.next().unwrap().scroll_y, 20.0);
        assert!(queue.next().is_none());
    }

    #[test]
    fn leaving_while_a_write_runs_drains_the_latest_position_without_overlap() {
        let mut queue = ProgressQueue::default();
        queue.record(update("a", 10.0));
        queue.start();
        assert_eq!(queue.next().unwrap().scroll_y, 10.0);
        queue.record(update("a", 20.0));
        queue.record(update("a", 30.0));
        queue.record(update("b", 40.0));
        assert!(!queue.start());
        assert_eq!(queue.next().unwrap().scroll_y, 30.0);
        assert_eq!(queue.latest["a"].scroll_y, 30.0);
        assert_eq!(queue.next().unwrap().item_id, "b");
        assert!(queue.next().is_none());
        queue.record(update("b", 50.0));
        assert!(queue.start());
    }

    #[test]
    fn close_retries_failed_positions_and_waits_for_the_latest_running_write() {
        let mut queue = ProgressQueue::default();
        queue.record(update("failed", 10.0));
        assert!(queue.start());
        let failed = queue.next().unwrap();
        assert!(queue.next().is_none());
        queue.retry_latest();
        assert!(queue.start());
        assert_eq!(queue.next(), Some(failed.clone()));
        queue.record(update("failed", 30.0));
        queue.retry_latest();
        assert!(!queue.start());
        queue.confirmed(&failed);
        assert_eq!(queue.latest["failed"].scroll_y, 30.0);
        let latest = queue.next().unwrap();
        queue.confirmed(&latest);
        assert!(queue.next().is_none());
        assert!(queue.latest.is_empty());
    }

    #[test]
    fn confirming_a_write_only_clears_its_own_snapshot() {
        let mut queue = ProgressQueue::default();
        queue.record(update("a", 10.0));
        queue.start();
        let old = queue.next().unwrap();
        queue.record(update("a", 20.0));
        queue.confirmed(&old);
        assert_eq!(queue.latest["a"].scroll_y, 20.0);
        let current = queue.next().unwrap();
        queue.confirmed(&current);
        assert!(!queue.latest.contains_key("a"));
    }
}
