//! Session projections keep pending saves and old list snapshots consistent.

use crate::api::{HighlightDetail, HighlightListRow, NoteDetail};
use leptos::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
struct Changes {
    removed: BTreeSet<String>,
    notes: BTreeMap<String, NoteDetail>,
    pending: BTreeSet<String>,
    superseded: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Copy)]
pub(crate) struct AnnotationActions(RwSignal<Changes>);

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl AnnotationActions {
    pub(crate) fn has_pending(self) -> bool {
        self.0.with(|state| !state.pending.is_empty())
    }
    pub(crate) fn pending(self, id: &str) -> bool {
        self.0.with(|state| state.pending.contains(id))
    }
    pub(crate) fn edit(self, mut note: NoteDetail, body: String) {
        note.body = body;
        self.0.update(|state| {
            state.pending.insert(note.id.clone());
            state.notes.insert(note.id.clone(), note);
        });
    }
    pub(crate) fn saved(self, note: NoteDetail) {
        self.0.update(|state| {
            state.pending.remove(&note.id);
            let predecessors = note
                .superseded_revision_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let linked = state
                .notes
                .get(&note.id)
                .is_some_and(|previous| predecessors.contains(&previous.revision_id));
            if linked {
                state
                    .superseded
                    .entry(note.id.clone())
                    .or_default()
                    .extend(predecessors);
            } else {
                state.superseded.insert(note.id.clone(), predecessors);
            }
            state.notes.insert(note.id.clone(), note);
        });
    }
    pub(crate) fn edit_failed(self, original: NoteDetail) {
        self.0.update(|state| {
            state.pending.remove(&original.id);
            state.notes.insert(original.id.clone(), original);
        });
    }
    pub(crate) fn remove(self, id: String) {
        self.0.update(|state| {
            state.pending.insert(id.clone());
            state.removed.insert(id);
        });
    }
    pub(crate) fn removed(self, id: &str) {
        self.0.update(|state| {
            state.pending.remove(id);
        });
    }
    pub(crate) fn removal_failed(self, id: &str) {
        self.0.update(|state| {
            state.pending.remove(id);
            state.removed.remove(id);
        });
    }
    pub(crate) fn project_rows(self, rows: Vec<HighlightListRow>) -> Vec<HighlightListRow> {
        self.0.with(|state| {
            rows.into_iter()
                .filter(|row| !state.removed.contains(&row.highlight.id))
                .map(|mut row| {
                    for note in &mut row.notes {
                        if let Some(changed) = state.notes.get(&note.id) {
                            let current_heads = head_ids(note);
                            if state.pending.contains(&note.id)
                                && current_heads == head_ids(changed)
                            {
                                note.body = changed.body.clone();
                            } else if !state.pending.contains(&note.id)
                                && state.superseded.get(&note.id).is_some_and(|retired| {
                                    current_heads.iter().all(|id| retired.contains(id))
                                })
                            {
                                let can_edit = note.can_edit;
                                *note = changed.clone();
                                note.can_edit = can_edit;
                            }
                        }
                    }
                    if let Some(note) = row.notes.first() {
                        let text = note.body.split_whitespace().collect::<Vec<_>>().join(" ");
                        row.note_excerpt = Some(if text.chars().count() > 180 {
                            format!("{}...", text.chars().take(180).collect::<String>())
                        } else {
                            text
                        });
                    }
                    row
                })
                .collect()
        })
    }
    pub(crate) fn project_highlights(self, rows: Vec<HighlightDetail>) -> Vec<HighlightDetail> {
        self.0.with(|state| {
            rows.into_iter()
                .filter(|row| !state.removed.contains(&row.id))
                .collect()
        })
    }
}

fn head_ids(note: &NoteDetail) -> BTreeSet<String> {
    if note.conflicts.is_empty() {
        BTreeSet::from([note.revision_id.clone()])
    } else {
        note.conflicts
            .iter()
            .map(|version| version.revision_id.clone())
            .collect()
    }
}

pub(crate) fn provide_annotation_actions() {
    provide_context(AnnotationActions(RwSignal::new(Changes::default())));
}
pub(crate) fn use_annotation_actions() -> AnnotationActions {
    use_context().unwrap_or_else(|| AnnotationActions(RwSignal::new(Changes::default())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};

    fn row() -> HighlightListRow {
        HighlightListRow {
            highlight: HighlightDetail {
                id: "highlight".into(),
                item_id: "item".into(),
                quoted_text: "quote".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: None,
                    exact: "quote".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 5,
                }),
                location_label: None,
                color: None,
                created_by: "device".into(),
                created_at: String::new(),
                created_at_micros: 1,
                signature: String::new(),
            },
            item_title: "Article".into(),
            item_source_name: "Example".into(),
            item_primary_url: None,
            note_excerpt: Some("Original".into()),
            notes: vec![NoteDetail {
                id: "note".into(),
                revision_id: "original".into(),
                superseded_revision_ids: Vec::new(),
                conflicts: Vec::new(),
                body: "Original".into(),
                target_kind: Some("highlight".into()),
                target_id: Some("highlight".into()),
                created_by: "device".into(),
                created_at: String::new(),
                created_at_micros: 1,
                updated_at_micros: 1,
                signature: String::new(),
                can_edit: true,
            }],
            can_remove: true,
        }
    }

    #[test]
    fn edits_project_over_stale_snapshots_roll_back_and_yield_to_newer_confirmed_data() {
        Owner::new().with(|| {
            let actions = use_annotation_actions();
            let original = row();
            let note = original.notes[0].clone();
            actions.edit(note.clone(), "Draft".into());
            assert!(actions.has_pending());
            assert_eq!(
                actions.project_rows(vec![original.clone()])[0].notes[0].body,
                "Draft"
            );
            actions.edit_failed(note.clone());
            assert!(!actions.has_pending());
            assert_eq!(
                actions.project_rows(vec![original.clone()])[0].notes[0],
                note
            );
            let mut saved = note.clone();
            saved.revision_id = "saved".into();
            saved.superseded_revision_ids = vec![note.revision_id.clone()];
            saved.updated_at_micros = 2;
            saved.body = "Saved".into();
            actions.edit(note, saved.body.clone());
            actions.saved(saved.clone());
            assert_eq!(
                actions.project_rows(vec![original.clone()])[0].notes[0],
                saved
            );
            let mut read_only = original.clone();
            read_only.notes[0].can_edit = false;
            let projected = actions.project_rows(vec![read_only]);
            assert_eq!(projected[0].notes[0].body, saved.body);
            assert!(!projected[0].notes[0].can_edit);
            let mut newer = original;
            newer.notes[0].revision_id = "elsewhere".into();
            newer.notes[0].updated_at_micros = 3;
            newer.notes[0].body = "Newer elsewhere".into();
            assert_eq!(
                actions.project_rows(vec![newer.clone()])[0].notes[0],
                newer.notes[0]
            );
        });
    }

    #[test]
    fn equal_time_remote_branches_are_visible_during_and_after_a_local_save() {
        Owner::new().with(|| {
            let actions = use_annotation_actions();
            let original = row();
            let mut saved = original.notes[0].clone();
            saved.revision_id = "local".into();
            saved.superseded_revision_ids = vec!["original".into()];
            saved.body = "Local draft".into();
            saved.updated_at_micros = 2;
            actions.edit(original.notes[0].clone(), saved.body.clone());
            let mut conflict = original.clone();
            conflict.notes[0].conflicts = vec![
                crate::api::NoteConflictVersion {
                    revision_id: "local".into(),
                    body: "Local draft".into(),
                    saved_at: String::new(),
                    device_label: "A".into(),
                },
                crate::api::NoteConflictVersion {
                    revision_id: "remote".into(),
                    body: "Remote draft".into(),
                    saved_at: String::new(),
                    device_label: "B".into(),
                },
            ];
            conflict.notes[0].revision_id = "local".into();
            conflict.notes[0].body = "Local draft".into();
            conflict.notes[0].updated_at_micros = 2;
            assert_eq!(
                actions.project_rows(vec![conflict.clone()])[0].notes,
                conflict.notes
            );
            actions.saved(saved.clone());
            assert_eq!(
                actions.project_rows(vec![conflict.clone()])[0].notes,
                conflict.notes
            );
            assert_eq!(
                actions.project_rows(vec![original.clone()])[0].notes[0],
                saved
            );
            let mut resolved = saved.clone();
            resolved.revision_id = "resolution".into();
            resolved.body = "Combined".into();
            resolved.superseded_revision_ids = vec!["local".into(), "remote".into()];
            actions.edit(conflict.notes[0].clone(), resolved.body.clone());
            actions.saved(resolved.clone());
            assert_eq!(actions.project_rows(vec![conflict])[0].notes[0], resolved);
            assert_eq!(actions.project_rows(vec![original])[0].notes[0], resolved);
        });
    }

    #[test]
    fn removal_rolls_back_on_failure_and_confirmed_removals_hide_reader_and_list_snapshots() {
        Owner::new().with(|| {
            let actions = use_annotation_actions();
            let original = row();
            actions.remove(original.highlight.id.clone());
            assert!(actions.project_rows(vec![original.clone()]).is_empty());
            assert!(
                actions
                    .project_highlights(vec![original.highlight.clone()])
                    .is_empty()
            );
            actions.removal_failed(&original.highlight.id);
            assert_eq!(
                actions.project_rows(vec![original.clone()]),
                vec![original.clone()]
            );
            actions.remove(original.highlight.id.clone());
            actions.removed(&original.highlight.id);
            assert!(!actions.has_pending());
            assert!(actions.project_rows(vec![original.clone()]).is_empty());
            assert!(
                actions
                    .project_highlights(vec![original.highlight])
                    .is_empty()
            );
        });
    }
}
