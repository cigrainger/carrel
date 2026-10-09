//! Coalesce scroll snapshots and serialize progress writes across route changes.

use crate::api::{self, ReadProgressUpdate};
use crate::feeds::{FeedController, use_feed_controller};
use crate::status::StatusController;
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
}

#[derive(Clone)]
pub(crate) struct ProgressWriter {
    queue: Arc<Mutex<ProgressQueue>>,
    status: StatusController,
    feeds: FeedController,
}

impl ProgressWriter {
    pub(crate) fn new(status: StatusController, feeds: FeedController) -> Self {
        Self {
            queue: Arc::default(),
            status,
            feeds,
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
        spawn_progress_task(async move {
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
