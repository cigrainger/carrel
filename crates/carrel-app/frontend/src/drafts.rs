//! App-owned composition queues. Native commands own persistence and reply rules.

use crate::{
    api,
    sharing::{ConversationDraft, request_token},
    writes::{PendingWrites, WriteGuard},
};
use leptos::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) struct DraftView {
    pub draft: Option<ConversationDraft>,
    pub loading: bool,
    pub unsaved: bool,
    pub busy: bool,
    pub discarding: bool,
    pub error: Option<String>,
    pub conflict: bool,
    pub reviewed: bool,
    pub saved: Option<api::ReplyDraft>,
    pub failed_send: bool,
    pub submitted: Option<ConversationDraft>,
    pub notice: Option<String>,
}

#[derive(Clone)]
enum Intent {
    Load {
        review: bool,
    },
    Send {
        submitted: ConversationDraft,
        retry: Option<api::ReplyDraftSnapshot>,
    },
    Discard(ConversationDraft),
}

enum Operation {
    Load {
        review: bool,
    },
    Save {
        revision: String,
        submitted: ConversationDraft,
    },
    Send {
        snapshot: api::ReplyDraftSnapshot,
        submitted: ConversationDraft,
    },
    Discard {
        revision: String,
        submitted: ConversationDraft,
    },
}

#[derive(Default)]
struct Entry {
    snapshot: Option<api::ReplyDraftSnapshot>,
    current: Option<ConversationDraft>,
    intent: Option<Intent>,
    running: bool,
    dirty: bool,
    error: Option<String>,
    conflict: bool,
    reviewed: bool,
    failed_send: Option<api::ReplyDraftSnapshot>,
    uncertain_send: bool,
    notice: Option<String>,
    timer: u64,
    guard: Option<WriteGuard>,
}

fn composition(draft: &api::ReplyDraft) -> ConversationDraft {
    ConversationDraft {
        body: draft.body.clone(),
        parent: draft.parent.clone(),
        request_id: draft.request_id.clone(),
    }
}

fn empty() -> ConversationDraft {
    ConversationDraft {
        body: String::new(),
        parent: None,
        request_id: request_token(),
    }
}

impl Entry {
    fn view(&self) -> DraftView {
        DraftView {
            draft: self.current.clone(),
            loading: self.snapshot.is_none() && self.running,
            unsaved: self.dirty,
            busy: matches!(self.intent, Some(Intent::Send { .. } | Intent::Discard(_))),
            discarding: matches!(self.intent, Some(Intent::Discard(_))),
            error: self.error.clone(),
            conflict: self.conflict,
            reviewed: self.reviewed,
            saved: self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.draft.clone()),
            failed_send: self.failed_send.is_some(),
            submitted: self
                .failed_send
                .as_ref()
                .and_then(|snapshot| snapshot.draft.as_ref())
                .map(composition),
            notice: self.notice.clone(),
        }
    }

    fn next(&self) -> Option<Operation> {
        if self.error.is_some() {
            return None;
        }
        match &self.intent {
            Some(Intent::Load { review }) => Some(Operation::Load { review: *review }),
            Some(Intent::Discard(submitted)) => Some(Operation::Discard {
                revision: self.snapshot.as_ref()?.revision.clone(),
                submitted: submitted.clone(),
            }),
            Some(Intent::Send { submitted, retry }) => {
                let snapshot = retry.as_ref().or(self.snapshot.as_ref())?;
                if retry.is_some()
                    || snapshot.draft.as_ref().map(composition).as_ref() == Some(submitted)
                {
                    Some(Operation::Send {
                        snapshot: snapshot.clone(),
                        submitted: submitted.clone(),
                    })
                } else {
                    Some(Operation::Save {
                        revision: snapshot.revision.clone(),
                        submitted: submitted.clone(),
                    })
                }
            }
            None if self.dirty && self.failed_send.is_none() => Some(Operation::Save {
                revision: self.snapshot.as_ref()?.revision.clone(),
                submitted: self.current.clone()?,
            }),
            None => None,
        }
    }

    fn saved(&mut self, operation: Operation, snapshot: api::ReplyDraftSnapshot) {
        self.error = None;
        self.conflict = false;
        self.reviewed = false;
        match operation {
            Operation::Load { review } => {
                if self.current.is_none() {
                    self.current = Some(
                        snapshot
                            .draft
                            .as_ref()
                            .map(composition)
                            .unwrap_or_else(empty),
                    );
                }
                self.intent = None;
                if review {
                    self.conflict = true;
                    self.reviewed = true;
                    self.error = Some("Review the saved draft below, then choose whether to keep your composition or discard it.".into());
                }
            }
            Operation::Save { submitted, .. } => {
                self.dirty = self.current.as_ref() != Some(&submitted);
            }
            Operation::Send { submitted, .. } => {
                self.intent = None;
                self.failed_send = None;
                self.uncertain_send = false;
                if let Some(saved) = snapshot.draft.as_ref() {
                    // A deduplicated Send may return a later native draft. It is
                    // never an acknowledgement of that later composition.
                    if self.current.as_ref() == Some(&submitted) {
                        self.current = Some(composition(saved));
                    }
                    if let Some(current) = &mut self.current
                        && current.request_id == submitted.request_id
                    {
                        current.request_id = request_token();
                    }
                    self.dirty = self.current.as_ref() != Some(&composition(saved));
                    if self.dirty
                        && self.current.as_ref().is_some_and(|current| {
                            current.body != saved.body || current.parent != saved.parent
                        })
                    {
                        self.conflict = true;
                        self.reviewed = true;
                        self.error = Some("Your reply was saved, and another draft is already stored. Review both before saving your newer composition.".into());
                    }
                } else if let Some(current) = &mut self.current {
                    let exact = *current == submitted;
                    current.acknowledge(&submitted, request_token);
                    self.dirty = !exact;
                }
                self.notice =
                    Some("Reply saved here. Delivery may wait while desktops are offline.".into());
            }
            Operation::Discard { submitted, .. } => {
                self.intent = None;
                self.failed_send = None;
                self.uncertain_send = false;
                if self.current.as_ref() == Some(&submitted) {
                    self.current = Some(empty());
                    self.dirty = false;
                } else {
                    if let Some(current) = &mut self.current {
                        current.request_id = request_token();
                    }
                    self.dirty = true;
                }
                self.notice = Some("Draft discarded on this desktop.".into());
            }
        }
        self.snapshot = Some(snapshot);
    }
}

/// One serial writer per thread, retained across dialog mount/unmount.
#[derive(Clone)]
pub(crate) struct DraftController {
    entries: Arc<Mutex<BTreeMap<String, Entry>>>,
    changed: RwSignal<u64>,
    history: RwSignal<u64>,
    writes: PendingWrites,
    status: crate::status::StatusController,
}

impl DraftController {
    pub(crate) fn new(history: RwSignal<u64>) -> Self {
        Self {
            entries: Arc::default(),
            changed: RwSignal::new(0),
            history,
            writes: crate::writes::use_pending_writes(),
            status: crate::status::use_status_controller(),
        }
    }

    fn notify(&self) {
        self.changed
            .try_update(|revision| *revision = revision.wrapping_add(1));
    }

    pub(crate) fn view(&self, id: &str) -> DraftView {
        self.changed.get();
        self.entries
            .lock()
            .expect("draft queue poisoned")
            .get(id)
            .map(Entry::view)
            .unwrap_or_default()
    }

    pub(crate) fn load(&self, id: &str) {
        {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let entry = entries.entry(id.into()).or_default();
            if entry.snapshot.is_some() || entry.running {
                return;
            }
            entry.error = None;
            entry.intent = Some(Intent::Load { review: false });
        }
        self.start(id);
    }

    pub(crate) fn edit(&self, id: &str, change: impl FnOnce(&mut ConversationDraft)) {
        let timer = {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            let Some(current) = &mut entry.current else {
                return;
            };
            let before = current.clone();
            change(current);
            if *current == before {
                return;
            }
            if entry.failed_send.is_some() && !entry.uncertain_send {
                // A native refusal queued nothing; editing can save normally.
                entry.failed_send = None;
                entry.error = None;
            }
            entry.dirty = true;
            entry.notice = None;
            if entry.error.is_some() || entry.failed_send.is_some() {
                None
            } else {
                entry.guard.get_or_insert_with(|| self.writes.begin());
                entry.timer = entry.timer.wrapping_add(1);
                Some(entry.timer)
            }
        };
        self.notify();
        if let Some(timer) = timer {
            #[cfg(target_arch = "wasm32")]
            {
                let writer = self.clone();
                let id = id.to_owned();
                set_timeout(
                    move || {
                        let current = writer
                            .entries
                            .lock()
                            .expect("draft queue poisoned")
                            .get(&id)
                            .is_some_and(|entry| entry.timer == timer);
                        if current {
                            writer.start(&id);
                        }
                    },
                    std::time::Duration::from_millis(300),
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = timer;
        }
    }

    pub(crate) fn send(&self, id: &str) {
        {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            if entry.intent.is_some() || entry.snapshot.is_none() {
                return;
            }
            let retry = entry.failed_send.clone();
            if entry.conflict && retry.is_none() {
                return;
            }
            let submitted = retry
                .as_ref()
                .and_then(|snapshot| snapshot.draft.as_ref())
                .map(composition)
                .or_else(|| entry.current.clone());
            let Some(submitted) = submitted.filter(|draft| !draft.body.trim().is_empty()) else {
                return;
            };
            entry.error = None;
            entry.notice = None;
            entry.intent = Some(Intent::Send { submitted, retry });
        }
        self.start(id);
    }

    pub(crate) fn discard(&self, id: &str, reviewed: &ConversationDraft) {
        {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            if entry.intent.is_some() || entry.snapshot.is_none() {
                return;
            }
            if entry.current.as_ref() != Some(reviewed) {
                entry.error =
                    Some("Your draft changed. Review it and choose Discard draft again.".into());
                drop(entries);
                self.notify();
                return;
            }
            entry.error = None;
            entry.notice = None;
            entry.intent = Some(Intent::Discard(reviewed.clone()));
        }
        self.start(id);
    }

    pub(crate) fn review(&self, id: &str) {
        {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            if entry.running {
                return;
            }
            entry.error = None;
            entry.intent = Some(Intent::Load { review: true });
        }
        self.start(id);
    }

    pub(crate) fn retry_save(&self, id: &str) {
        let loaded = {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            if entry.failed_send.is_some() || (entry.conflict && !entry.reviewed) {
                return;
            }
            entry.error = None;
            entry.conflict = false;
            entry.reviewed = false;
            entry.snapshot.is_some()
        };
        if loaded {
            self.start(id);
        } else {
            self.load(id);
        }
    }

    /// Close and backup may retry persistence, never an intentional Send.
    pub(crate) fn flush_for_close(&self) {
        let ids = {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            entries
                .iter_mut()
                .filter_map(|(id, entry)| {
                    if entry.dirty && entry.failed_send.is_none() && !entry.conflict {
                        entry.error = None;
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.start(&id);
        }
    }

    pub(crate) fn has_unsaved_drafts(&self) -> bool {
        self.entries
            .lock()
            .expect("draft queue poisoned")
            .values()
            .any(|entry| {
                entry.dirty
                    || matches!(entry.intent, Some(Intent::Send { .. } | Intent::Discard(_)))
            })
    }

    fn start(&self, id: &str) {
        let launch = {
            let mut entries = self.entries.lock().expect("draft queue poisoned");
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            entry.timer = entry.timer.wrapping_add(1);
            if entry.running || entry.error.is_some() {
                false
            } else {
                entry.guard.get_or_insert_with(|| self.writes.begin());
                entry.running = true;
                true
            }
        };
        self.notify();
        if launch {
            let writer = self.clone();
            let id = id.to_owned();
            spawn(async move {
                writer.drain(id).await;
            });
        }
    }

    async fn drain(&self, id: String) {
        loop {
            let (operation, completed_guard) = {
                let mut entries = self.entries.lock().expect("draft queue poisoned");
                let entry = entries.get_mut(&id).expect("mounted draft");
                let operation = entry.next();
                if operation.is_none() {
                    entry.running = false;
                    (operation, entry.guard.take())
                } else {
                    (operation, None)
                }
            };
            drop(completed_guard);
            let Some(operation) = operation else {
                self.notify();
                return;
            };
            let result = match &operation {
                Operation::Load { .. } => api::load_reply_draft(&id).await,
                Operation::Save {
                    revision,
                    submitted,
                } => {
                    api::save_reply_draft(
                        &id,
                        revision,
                        &submitted.body,
                        &submitted.parent,
                        &submitted.request_id,
                    )
                    .await
                }
                Operation::Send { snapshot, .. } => api::send_reply_draft(&id, snapshot).await,
                Operation::Discard { revision, .. } => {
                    api::discard_reply_draft(&id, revision).await
                }
            };
            let sent = matches!(operation, Operation::Send { .. }) && result.is_ok();
            let failure = {
                let mut entries = self.entries.lock().expect("draft queue poisoned");
                let entry = entries.get_mut(&id).expect("mounted draft");
                match result {
                    Ok(snapshot) => {
                        entry.saved(operation, snapshot);
                        None
                    }
                    Err(problem) => {
                        let action = match &operation {
                            Operation::Load { .. } => "load your reply draft",
                            Operation::Save { .. } => "save your reply draft",
                            Operation::Send {
                                snapshot,
                                submitted,
                            } => {
                                entry.uncertain_send = !problem.is_draft_send_refusal();
                                entry.dirty = entry.uncertain_send
                                    || problem.is_draft_conflict()
                                    || entry.current.as_ref() != Some(submitted);
                                entry.failed_send = if !problem.is_draft_conflict()
                                    && (entry.uncertain_send || !entry.dirty)
                                {
                                    Some(snapshot.clone())
                                } else {
                                    None
                                };
                                "confirm your reply was sent"
                            }
                            Operation::Discard { .. } => "discard your reply draft",
                        };
                        if matches!(
                            operation,
                            Operation::Save { .. } | Operation::Discard { .. }
                        ) {
                            entry.dirty = true;
                        }
                        entry.conflict = problem.is_draft_conflict()
                            || (matches!(operation, Operation::Load { review: true })
                                && entry.conflict);
                        entry.reviewed = false;
                        let message = format!("Could not {action}. {}", problem.message());
                        entry.error = Some(message.clone());
                        // Never let a close-time persistence retry become a Send.
                        entry.intent = None;
                        Some(message)
                    }
                }
            };
            self.notify();
            if sent {
                self.history.try_update(|revision| *revision += 1);
            }
            if let Some(message) = failure {
                self.status.warn(message);
            }
        }
    }
}

fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    leptos::task::spawn_local(future);
    #[cfg(not(target_arch = "wasm32"))]
    drop(future);
}
