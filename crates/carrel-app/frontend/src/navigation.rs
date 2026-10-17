//! Shared route-local navigation context.

use crate::api::{AnnotationCursor, HighlightListRow, ItemCursor, ItemSummary};
use leptos::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

/// Open a saved article from a dialog without losing the list behind it.
pub(crate) fn use_article_opener() -> UnsyncCallback<String> {
    use leptos_router::{
        NavigateOptions,
        hooks::{use_location, use_navigate},
    };
    let location = use_location();
    let navigate = use_navigate();
    let navigation = use_list_navigation_context();
    let (_, remember) = crate::keymap::action("remember-list-context");
    UnsyncCallback::new(move |id: String| {
        remember.run(());
        let mut origin = location.pathname.get_untracked();
        let query = location.search.get_untracked();
        if !query.is_empty() {
            origin.push('?');
            origin.push_str(&query);
        }
        navigation.enter_link(&id, &origin);
        navigate(
            &format!("/item/{id}"),
            NavigateOptions {
                scroll: false,
                ..Default::default()
            },
        );
    })
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ListSnapshot {
    pub(crate) items: Vec<ItemSummary>,
    pub(crate) highlights: Vec<HighlightListRow>,
    pub(crate) selected_id: Option<String>,
    pub(crate) selected_index: usize,
    pub(crate) scroll_y: i32,
    pub(crate) item_cursor: Option<ItemCursor>,
    pub(crate) items_complete: bool,
    pub(crate) annotation_cursor: Option<AnnotationCursor>,
    pub(crate) annotations_complete: bool,
    pub(crate) expanded_highlights: BTreeSet<String>,
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

/// Session visibility of the shared navigation sidebar.
#[derive(Clone, Copy)]
pub(crate) struct ReadingSidebar(
    pub(crate) RwSignal<bool>,
    #[cfg(target_arch = "wasm32")] crate::preferences::PreferencesController,
);

impl ReadingSidebar {
    pub(crate) fn toggle(self) {
        #[cfg(target_arch = "wasm32")]
        self.1
            .preserve_layout(|| self.0.update(|visible| *visible = !*visible));
        #[cfg(not(target_arch = "wasm32"))]
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
    selection_before_link: RwSignal<bool>,
}

impl ListNavigationContext {
    /// Keep a useful return destination without inventing a stale next/previous queue.
    pub(crate) fn enter_link(&self, id: &str, origin: &str) {
        let preserve = self.selection_before_link.get_untracked() || !origin.starts_with("/item/");
        self.set(navigation_for_link(self.get(), id, origin));
        self.selection_before_link.set(preserve);
    }
    pub fn set(&self, navigation: ListNavigation) {
        self.current.set(Some(navigation));
        self.selection_before_link.set(false);
    }

    /// An external detour keeps the list cursor until explicit reader browsing.
    pub(crate) fn preserve_return_selection(&self) -> bool {
        self.selection_before_link.get_untracked()
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

fn navigation_for_link(previous: Option<ListNavigation>, id: &str, origin: &str) -> ListNavigation {
    if let Some(mut previous) = previous
        && (origin.starts_with("/item/") || previous.return_path == origin)
    {
        if !previous.item_ids.iter().any(|item| item == id) {
            previous.item_ids = vec![id.into()];
        }
        previous.current_id = id.into();
        return previous;
    }
    let path = origin.split('?').next().unwrap_or(origin);
    let (return_path, return_label) = match path {
        "/" | "/today" => (origin, "Today"),
        "/reading" => (origin, "Reading"),
        "/library" | "/unread" | "/starred" => (origin, "Library"),
        "/highlights" => (origin, "Highlights"),
        "/friends" => (origin, "Shared with you"),
        path if path.starts_with("/feeds/") => (origin, "Source"),
        path if path.starts_with("/folders/") => (origin, "Folder"),
        _ => ("/library", "Library"),
    };
    ListNavigation {
        item_ids: vec![id.into()],
        current_id: id.into(),
        return_path: return_path.into(),
        return_label: return_label.into(),
    }
}

/// Provide list navigation context for route children.
pub fn provide_list_navigation_context() -> ListNavigationContext {
    #[cfg(target_arch = "wasm32")]
    let sidebar = ReadingSidebar(RwSignal::new(true), crate::preferences::use_preferences());
    #[cfg(not(target_arch = "wasm32"))]
    let sidebar = ReadingSidebar(RwSignal::new(true));
    provide_context(sidebar);
    let context = ListNavigationContext {
        current: RwSignal::new(None),
        snapshots: RwSignal::new(BTreeMap::new()),
        selection_before_link: RwSignal::new(false),
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

#[cfg(target_arch = "wasm32")]
pub(crate) fn list_has_focus() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .is_some_and(|element| {
            element
                .closest(".item-row, .highlight-row")
                .ok()
                .flatten()
                .is_some()
        })
}

pub(crate) fn restore_list_scroll(path: String, y: i32, restore_focus: bool) {
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
                if restore_focus
                    && let Ok(Some(button)) = element
                        .query_selector(".is-selected .item-open, .is-selected .highlight-open")
                    && let Ok(button) = button.dyn_into::<web_sys::HtmlElement>()
                {
                    let options = web_sys::FocusOptions::new();
                    options.set_prevent_scroll(true);
                    let _ = button.focus_with_options(&options);
                }
                element.set_scroll_top(y);
            }
        });
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 0);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (path, y, restore_focus);
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
    fn an_external_detour_retains_list_selection_until_explicit_browsing() {
        Owner::new().with(|| {
            let context = provide_list_navigation_context();
            let original = navigation_for_link(None, "a", "/library?q=essay");
            context.set(original.clone());
            context.enter_link("b", "/library?q=essay");
            assert!(context.preserve_return_selection());
            assert_eq!(context.get().unwrap().current_id, "b");
            context.enter_link("c", "/item/b");
            assert!(context.preserve_return_selection());
            context.set(original);
            assert!(!context.preserve_return_selection());
            context.enter_link("b", "/item/a");
            assert!(!context.preserve_return_selection());
        });
    }

    #[test]
    fn incoming_links_preserve_reader_origin_or_the_current_filtered_list() {
        let first = navigation_for_link(None, "a", "/library?q=essay");
        assert_eq!(first.return_path, "/library?q=essay");
        let old = ListNavigation {
            item_ids: vec!["a".into(), "b".into()],
            ..first
        };
        let same_queue = navigation_for_link(Some(old.clone()), "b", "/item/a");
        assert_eq!(same_queue.current_id, "b");
        assert_eq!(same_queue.item_ids, old.item_ids);
        let outside_queue = navigation_for_link(Some(old.clone()), "c", "/item/a");
        assert_eq!(outside_queue.item_ids, vec!["c"]);
        assert_eq!(outside_queue.return_path, old.return_path);
        let new_list = navigation_for_link(Some(old), "d", "/highlights");
        assert_eq!(new_list.return_path, "/highlights");
        assert_eq!(
            navigation_for_link(None, "a", "/item/b").return_path,
            "/library"
        );
    }

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
    fn live_refresh_inserts_new_articles_without_moving_the_selected_identity() {
        let old = ["essay".into(), "post".into()];
        let refreshed = [
            "new-a".into(),
            "new-b".into(),
            "essay".into(),
            "post".into(),
        ];
        let selected = restored_index(&refreshed, old.first().map(String::as_str), 0, &old);
        assert_eq!(selected, 2);
        assert_eq!(refreshed[selected], "essay");
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
