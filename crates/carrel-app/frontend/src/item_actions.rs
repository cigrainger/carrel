//! Optimistic presentation state for serialized local item commands.

use std::collections::{BTreeMap, VecDeque};

use leptos::prelude::*;

use crate::api::{self, ItemStateRequest};
use crate::feeds::{FeedController, use_feed_controller};
use crate::status::{StatusController, use_status_controller};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ItemState {
    pub(crate) read: String,
    pub(crate) starred: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ItemAction {
    Read(bool),
    Star,
}

impl ItemAction {
    fn apply(self, state: &mut ItemState) {
        match self {
            Self::Read(read) => state.read = if read { "read" } else { "unread" }.into(),
            Self::Star => state.starred = !state.starred,
        }
    }
}

#[derive(Clone, Debug)]
struct PendingAction {
    item_id: String,
    action: ItemAction,
    starred: Option<bool>,
}

#[derive(Clone, Debug)]
struct ConfirmedState {
    value: ItemState,
    read_changed: bool,
    star_changed: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ItemActionState {
    confirmed: BTreeMap<String, ConfirmedState>,
    pending: VecDeque<PendingAction>,
    revision: u64,
}

impl ItemActionState {
    fn enqueue(&mut self, item_id: String, initial: ItemState, action: ItemAction) -> bool {
        self.revision += 1;
        let start = self.pending.is_empty();
        let starred = matches!(action, ItemAction::Star)
            .then(|| !self.project(&item_id, initial.clone()).starred);
        self.confirmed
            .entry(item_id.clone())
            .or_insert(ConfirmedState {
                value: initial,
                read_changed: false,
                star_changed: false,
            });
        self.pending.push_back(PendingAction {
            item_id,
            action,
            starred,
        });
        start
    }

    pub(crate) fn project(&self, item_id: &str, initial: ItemState) -> ItemState {
        let mut state = initial;
        if let Some(confirmed) = self.confirmed.get(item_id) {
            if confirmed.read_changed {
                state.read.clone_from(&confirmed.value.read);
            }
            if confirmed.star_changed {
                state.starred = confirmed.value.starred;
            }
        }
        for pending in self
            .pending
            .iter()
            .filter(|pending| pending.item_id == item_id)
        {
            if let Some(starred) = pending.starred {
                state.starred = starred;
            } else {
                pending.action.apply(&mut state);
            }
        }
        state
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn is_pending(&self, item_id: &str) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.item_id == item_id)
    }

    fn finish(&mut self, result: Result<Option<bool>, ()>) {
        let Some(pending) = self.pending.pop_front() else {
            return;
        };
        self.revision += 1;
        if let Ok(starred) = result {
            let state = self
                .confirmed
                .get_mut(&pending.item_id)
                .expect("queued item has a base state");
            pending.action.apply(&mut state.value);
            match pending.action {
                ItemAction::Read(_) => state.read_changed = true,
                ItemAction::Star => state.star_changed = true,
            }
            if let Some(starred) = starred {
                state.value.starred = starred;
            }
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// A query begun after the last write can become the presentation base again.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn rebase(&mut self, revision: u64, ids: &[String]) {
        if self.revision != revision {
            return;
        }
        for id in ids {
            if !self.is_pending(id) {
                self.confirmed.remove(id);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ItemActions {
    pub(crate) state: RwSignal<ItemActionState>,
    status: StatusController,
    feeds: FeedController,
    writes: StoredValue<crate::writes::PendingWrites>,
}

impl ItemActions {
    pub(crate) fn request(self, item_id: String, initial: ItemState, action: ItemAction) {
        let start = self
            .state
            .try_update(|state| state.enqueue(item_id, initial, action))
            .unwrap_or(false);
        if !start {
            return;
        }
        let status = self.status;
        let guard = self.writes.get_value().begin();
        spawn_action_task(async move {
            let _guard = guard;
            while let Some(pending) = self
                .state
                .try_with_untracked(|state| state.pending.front().cloned())
                .flatten()
            {
                let request = ItemStateRequest {
                    item_id: pending.item_id,
                };
                let result = match pending.action {
                    ItemAction::Read(true) => api::mark_item_read(request).await.map(|()| None),
                    ItemAction::Read(false) => api::mark_item_unread(request).await.map(|()| None),
                    ItemAction::Star => api::set_item_star(
                        request.item_id,
                        pending.starred.expect("star action has explicit intent"),
                    )
                    .await
                    .map(Some),
                };
                if result.is_err() {
                    status.warn("Could not save that change. Your previous state was restored.");
                }
                self.state
                    .try_update(|state| state.finish(result.map_err(|_| ())));
            }
            self.feeds.bump_refresh();
        });
    }
}

pub(crate) fn provide_item_actions() {
    provide_context(ItemActions {
        state: RwSignal::new(ItemActionState::default()),
        status: use_status_controller(),
        feeds: use_feed_controller(),
        writes: StoredValue::new(crate::writes::use_pending_writes()),
    });
}

pub(crate) fn use_item_actions() -> ItemActions {
    use_context::<ItemActions>().expect("ItemActions should be mounted")
}

#[cfg(target_arch = "wasm32")]
fn spawn_action_task(task: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_action_task(_task: impl std::future::Future<Output = ()> + 'static) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial() -> ItemState {
        ItemState {
            read: "unread".into(),
            starred: true,
        }
    }

    #[test]
    fn failed_unstar_restores_a_row_removed_from_the_starred_filter() {
        let mut state = ItemActionState::default();
        assert!(state.enqueue("one".into(), initial(), ItemAction::Star));
        assert!(!state.project("one", initial()).starred);
        state.finish(Err(()));
        assert!(state.project("one", initial()).starred);
        assert!(!state.is_pending("one"));
    }

    #[test]
    fn failed_read_preserves_a_later_unread_intent() {
        let mut state = ItemActionState::default();
        state.enqueue("one".into(), initial(), ItemAction::Read(true));
        assert!(!state.enqueue("one".into(), initial(), ItemAction::Read(false)));
        state.finish(Err(()));
        assert_eq!(state.project("one", initial()).read, "unread");
        state.finish(Ok(None));
        assert_eq!(state.project("one", initial()).read, "unread");
    }

    #[test]
    fn rapid_toggles_replay_pending_intents_after_a_failure() {
        let mut state = ItemActionState::default();
        state.enqueue("one".into(), initial(), ItemAction::Star);
        state.enqueue("one".into(), initial(), ItemAction::Star);
        assert!(state.project("one", initial()).starred);
        state.finish(Ok(Some(false)));
        assert!(state.project("one", initial()).starred);
        state.finish(Err(()));
        assert!(!state.project("one", initial()).starred);
    }

    #[test]
    fn independent_items_and_fields_reconcile_without_losing_each_other() {
        let mut state = ItemActionState::default();
        state.enqueue("one".into(), initial(), ItemAction::Read(true));
        state.enqueue("two".into(), initial(), ItemAction::Star);
        state.enqueue("one".into(), initial(), ItemAction::Star);
        state.finish(Ok(None));
        state.finish(Err(()));
        state.finish(Ok(Some(false)));
        assert_eq!(
            state.project("one", initial()),
            ItemState {
                read: "read".into(),
                starred: false
            }
        );
        assert_eq!(state.project("two", initial()), initial());
        assert!(state.pending.is_empty());
    }

    #[test]
    fn starring_does_not_override_new_read_progress_from_the_store() {
        let mut state = ItemActionState::default();
        state.enqueue("one".into(), initial(), ItemAction::Star);
        state.finish(Ok(Some(false)));
        let refreshed = ItemState {
            read: "reading".into(),
            starred: false,
        };
        assert_eq!(state.project("one", refreshed.clone()), refreshed);
    }

    #[test]
    fn stale_queries_cannot_erase_a_saved_action_but_later_queries_are_authoritative() {
        let mut state = ItemActionState::default();
        let stale_query = state.revision();
        state.enqueue("one".into(), initial(), ItemAction::Read(true));
        state.finish(Ok(None));
        state.rebase(stale_query, &["one".into()]);
        assert_eq!(state.project("one", initial()).read, "read");
        let fresh_query = state.revision();
        state.rebase(fresh_query, &["one".into()]);
        assert_eq!(state.project("one", initial()).read, "unread");
    }
    #[test]
    fn a_queued_star_keeps_the_clicked_intent_when_a_remote_snapshot_changes() {
        let mut actions = ItemActionState::default();
        actions.enqueue(
            "article".into(),
            ItemState {
                read: "unread".into(),
                starred: false,
            },
            ItemAction::Star,
        );
        assert_eq!(actions.pending.front().unwrap().starred, Some(true));
        assert!(
            actions
                .project(
                    "article",
                    ItemState {
                        read: "unread".into(),
                        starred: true
                    }
                )
                .starred
        );
        actions.enqueue(
            "article".into(),
            ItemState {
                read: "unread".into(),
                starred: true,
            },
            ItemAction::Star,
        );
        assert_eq!(actions.pending.back().unwrap().starred, Some(false));
        assert!(
            !actions
                .project(
                    "article",
                    ItemState {
                        read: "unread".into(),
                        starred: true
                    }
                )
                .starred
        );
    }
}
