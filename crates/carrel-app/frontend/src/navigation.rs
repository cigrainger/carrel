//! Shared route-local navigation context.

use crate::api::{HighlightListRow, ItemSummary};
use leptos::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub(crate) struct ListSnapshot {
    pub(crate) items: Vec<ItemSummary>,
    pub(crate) highlights: Vec<HighlightListRow>,
    pub(crate) selected_id: Option<String>,
    pub(crate) selected_index: usize,
    pub(crate) scroll_y: i32,
}

/// Keep the same selection, or its nearest remaining neighbour after a filter change.
pub(crate) fn restored_index(
    ids: &[String],
    selected: Option<&str>,
    previous: usize,
    original: &[String],
) -> usize {
    if let Some(index) = selected.and_then(|selected| ids.iter().position(|id| id == selected)) {
        return index;
    }
    let old_index = selected
        .and_then(|selected| original.iter().position(|id| id == selected))
        .unwrap_or(previous);
    if let Some((_, index)) = original
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            ids.iter()
                .position(|visible| visible == id)
                .map(|visible| ((index.abs_diff(old_index), index < old_index), visible))
        })
        .min_by_key(|(distance, _)| *distance)
    {
        return index;
    }
    previous.min(ids.len().saturating_sub(1))
}

/// Session choice for the reader's navigation sidebar.
#[derive(Clone, Copy)]
pub(crate) struct ReadingSidebar(pub(crate) RwSignal<bool>);

impl ReadingSidebar {
    pub(crate) fn toggle(self) {
        self.0.update(|visible| *visible = !*visible);
    }
}

/// The list context that opened an item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListNavigation {
    pub item_ids: Vec<String>,
    pub current_id: String,
    pub return_path: String,
    pub return_label: String,
}

/// Reading-route navigation state shared through Leptos context.
#[derive(Clone, Copy)]
pub struct ListNavigationContext {
    current: RwSignal<Option<ListNavigation>>,
    snapshots: RwSignal<BTreeMap<String, ListSnapshot>>,
}

impl ListNavigationContext {
    pub fn set(&self, navigation: ListNavigation) {
        self.current.set(Some(navigation));
    }

    pub fn get(&self) -> Option<ListNavigation> {
        self.current.get_untracked()
    }

    pub(crate) fn remember(&self, path: String, snapshot: ListSnapshot) {
        self.snapshots.update(|snapshots| {
            snapshots.insert(path, snapshot);
        });
    }

    pub(crate) fn snapshot(&self, path: &str) -> Option<ListSnapshot> {
        self.snapshots
            .with_untracked(|snapshots| snapshots.get(path).cloned())
    }
}

/// Provide list navigation context for route children.
pub fn provide_list_navigation_context() -> ListNavigationContext {
    provide_context(ReadingSidebar(RwSignal::new(false)));
    let context = ListNavigationContext {
        current: RwSignal::new(None),
        snapshots: RwSignal::new(BTreeMap::new()),
    };
    provide_context(context);
    context
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn list_scroll_y() -> i32 {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("main-content"))
        .map_or(0, |element| element.scroll_top())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn list_scroll_y() -> i32 {
    0
}

pub(crate) fn restore_list_scroll(path: String, y: i32) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::{JsCast, closure::Closure};
        let callback = Closure::once_into_js(move || {
            let document = web_sys::window().and_then(|window| window.document());
            if document
                .as_ref()
                .and_then(|document| document.query_selector("[data-list-path]").ok().flatten())
                .and_then(|list| list.get_attribute("data-list-path"))
                .as_deref()
                != Some(path.as_str())
            {
                return;
            }
            if let Some(element) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("main-content"))
            {
                element.set_scroll_top(y);
            }
        });
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 0);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (path, y);
}

pub(crate) fn ensure_list_cursor_visible(path: String) {
    #[cfg(target_arch = "wasm32")]
    set_timeout(
        move || {
            let Some(list) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.query_selector("[data-list-path]").ok().flatten())
            else {
                return;
            };
            if list.get_attribute("data-list-path").as_deref() != Some(path.as_str()) {
                return;
            }
            if let Ok(Some(row)) = list.query_selector(".is-selected") {
                use wasm_bindgen::JsCast;
                if let Ok(Some(button)) = row.query_selector(".item-open, .highlight-open")
                    && let Ok(button) = button.dyn_into::<web_sys::HtmlElement>()
                {
                    let _ = button.focus();
                }
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                row.scroll_into_view_with_scroll_into_view_options(&options);
            }
        },
        std::time::Duration::ZERO,
    );
    #[cfg(not(target_arch = "wasm32"))]
    let _ = path;
}

pub(crate) fn use_reading_sidebar() -> ReadingSidebar {
    use_context::<ReadingSidebar>().expect("ReadingSidebar should be mounted")
}

/// Read the current list navigation context.
pub fn use_list_navigation_context() -> ListNavigationContext {
    use_context::<ListNavigationContext>().expect("ListNavigationContext should be mounted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restoration_finds_the_selected_item_after_reordering() {
        assert_eq!(
            restored_index(
                &["b".into(), "a".into()],
                Some("a"),
                0,
                &["a".into(), "b".into()]
            ),
            1
        );
    }

    #[test]
    fn removed_selection_uses_a_neighbour_and_empty_lists_are_safe() {
        let original = ["a".into(), "b".into(), "c".into()];
        assert_eq!(
            restored_index(&["a".into(), "c".into()], Some("b"), 1, &original),
            1
        );
        assert_eq!(restored_index(&["a".into()], Some("b"), 9, &original), 0);
        assert_eq!(restored_index(&[], Some("b"), 9, &original), 0);
    }

    #[test]
    fn restoration_follows_the_original_neighbour_when_other_rows_also_left_the_filter() {
        let original = ["a".into(), "b".into(), "c".into(), "d".into(), "e".into()];
        assert_eq!(
            restored_index(&["b".into(), "e".into()], Some("d"), 0, &original),
            1
        );
        assert_eq!(
            restored_index(&["a".into(), "b".into()], Some("e"), 0, &original),
            1
        );
        assert_eq!(
            restored_index(&["a".into(), "e".into()], Some("b"), 1, &original),
            0
        );
    }
}
