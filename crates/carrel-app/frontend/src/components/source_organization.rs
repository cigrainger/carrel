//! Sidebar organization projects pending intent over authoritative subscriptions.

use std::collections::BTreeMap;
use std::future::Future;

use leptos::{ev, prelude::*};

use crate::api::FeedSummary;
use crate::feeds::{self, FeedController, use_feed_controller};
use crate::requests::Requests;
use crate::status::use_status_controller;

use super::Modal;

#[derive(Clone)]
enum Dialog {
    NewFolder,
    Source(FeedSummary),
}

#[derive(Clone, Copy)]
pub(super) struct Organization {
    controller: FeedController,
    pub(super) feeds: Signal<Vec<FeedSummary>>,
    pending: RwSignal<BTreeMap<String, Option<String>>>,
    dialog: RwSignal<Option<Dialog>>,
    dragged: RwSignal<Option<String>>,
}

pub(super) fn provide() -> Organization {
    let controller = use_feed_controller();
    let pending = RwSignal::new(BTreeMap::new());
    let feeds = Signal::derive(move || project(controller.feeds().get(), &pending.get()));
    let organization = Organization {
        controller,
        feeds,
        pending,
        dialog: RwSignal::new(None),
        dragged: RwSignal::new(None),
    };
    provide_context(organization);
    organization
}

fn project(
    mut feeds: Vec<FeedSummary>,
    pending: &BTreeMap<String, Option<String>>,
) -> Vec<FeedSummary> {
    for feed in &mut feeds {
        if let Some(folder) = pending.get(&feed.url) {
            feed.folder.clone_from(folder);
        }
    }
    feeds
}

impl Organization {
    pub(super) fn new_folder(self) {
        self.dialog.set(Some(Dialog::NewFolder));
    }

    pub(super) fn organize(self, source: FeedSummary) {
        self.dialog.set(Some(Dialog::Source(source)));
    }

    pub(super) fn busy(self, url: &str) -> bool {
        self.pending.with(|pending| pending.contains_key(url))
    }

    pub(super) fn dragging(self) -> bool {
        self.dragged.get().is_some()
    }

    pub(super) fn start_drag(self, event: ev::DragEvent, url: String) {
        #[cfg(target_arch = "wasm32")]
        {
            if self.busy(&url) {
                event.prevent_default();
                return;
            }
            if let Some(data) = event.data_transfer()
                && data.set_data("application/x-carrel-source", &url).is_ok()
            {
                data.set_effect_allowed("move");
                self.dragged.set(Some(url));
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (event, url);
    }

    pub(super) fn end_drag(self) {
        self.dragged.set(None);
    }

    async fn move_source(self, url: String, folder: String) -> Result<(), String> {
        if self.busy(&url) {
            return Err("This source is already being moved.".into());
        }
        let folder = folder.trim().to_string();
        let target = (!folder.is_empty()).then(|| folder.clone());
        self.pending.update(|pending| {
            pending.insert(url.clone(), target);
        });
        let result = feeds::set_folder(self.controller, url.clone(), folder).await;
        // Removing the projection reveals the latest native state, including
        // sync changes that arrived while a failed local write was pending.
        self.pending.try_update(|pending| {
            pending.remove(&url);
        });
        result
    }
}

/// Folder headings and Unfiled accept only this window's current source drag.
#[component]
pub(super) fn DropTarget(folder: Option<String>, children: Children) -> impl IntoView {
    let organization = expect_context::<Organization>();
    let folder = StoredValue::new(folder);
    let over = RwSignal::new(false);
    let status = use_status_controller();
    view! {
        <div class="source-drop-target" class:source-drop-over=move || over.get() && organization.dragging()
            on:dragover=move |event: ev::DragEvent| {
                if organization.dragging() {
                    event.prevent_default();
                    over.set(true);
                    #[cfg(target_arch = "wasm32")]
                    if let Some(data) = event.data_transfer() { data.set_drop_effect("move"); }
                }
            }
            on:dragleave=move |_| over.set(false)
            on:drop=move |event: ev::DragEvent| {
                over.set(false);
                #[cfg(target_arch = "wasm32")]
                if let Some(url) = organization.dragged.get_untracked()
                    && event.data_transfer().and_then(|data| data.get_data("application/x-carrel-source").ok()).as_deref() == Some(url.as_str())
                    && organization.controller.feeds().with_untracked(|feeds| feeds.iter().any(|feed| feed.url == url))
                {
                    event.prevent_default();
                    organization.end_drag();
                    let target = folder.get_value().unwrap_or_default();
                    spawn(async move {
                        match organization.move_source(url, target.clone()).await {
                            Ok(()) => status.info(if target.is_empty() { "Source moved to Unfiled".into() } else { format!("Source moved to {target}") }),
                            Err(message) => status.warn(format!("Could not move source: {message}")),
                        }
                    });
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (&event, folder, status);
            }>
            {children()}
        </div>
    }
}

#[component]
pub(super) fn OrganizationDialog() -> impl IntoView {
    let organization = expect_context::<Organization>();
    view! { <Show when=move || organization.dialog.get().is_some()>{move || organization.dialog.get().map(|dialog| view! { <FolderDialog dialog=dialog/> })}</Show> }
}

#[component]
fn FolderDialog(dialog: Dialog) -> impl IntoView {
    let organization = expect_context::<Organization>();
    let status = use_status_controller();
    let source = match &dialog {
        Dialog::Source(source) => Some(source.clone()),
        Dialog::NewFolder => None,
    };
    let creating = RwSignal::new(source.is_none());
    let selected_url = RwSignal::new(
        source
            .as_ref()
            .map(|source| source.url.clone())
            .unwrap_or_default(),
    );
    let selected_folder = RwSignal::new(
        source
            .as_ref()
            .and_then(|source| source.folder.clone())
            .unwrap_or_default(),
    );
    let new_name = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let folders = Memo::new(move |_| {
        organization
            .feeds
            .get()
            .into_iter()
            .filter_map(|feed| feed.folder)
            .collect::<std::collections::BTreeSet<_>>()
    });
    let close = UnsyncCallback::new(move |()| organization.dialog.set(None));
    if let Some(source) = &source {
        let url = source.url.clone();
        Effect::new(move |_| {
            if !organization
                .controller
                .feeds()
                .with(|feeds| feeds.iter().any(|feed| feed.url == url))
                && organization.controller.remove_target.get().is_none()
            {
                close.run(());
                focus_source(&url);
            }
        });
    }
    let remove = source.clone();
    view! {
        <Modal label="Organize sources" class="feed-dialog source-organization-dialog" on_cancel=close>
            <form on:submit=move |event| {
                event.prevent_default();
                if busy.get_untracked() { return; }
                let url = selected_url.get_untracked();
                let folder = if creating.get_untracked() { new_name.get_untracked().trim().to_string() } else { selected_folder.get_untracked() };
                if url.is_empty() { error.set(Some("Choose a source for this folder.".into())); return; }
                if creating.get_untracked() && folder.is_empty() { error.set(Some("Enter a folder name.".into())); return; }
                if creating.get_untracked() && folders.get_untracked().contains(&folder) { error.set(Some("That folder already exists. Choose a different name.".into())); return; }
                let guard = requests.get_value();
                let request = guard.begin();
                busy.set(true);
                error.set(None);
                spawn(async move {
                    let result = organization.move_source(url.clone(), folder).await;
                    if !guard.accepts(request) {
                        if let Err(message) = result { status.warn(format!("Could not move source: {message}")); }
                        return;
                    }
                    busy.try_set(false);
                    match result {
                        Ok(()) => { close.run(()); focus_source(&url); status.info("Source folder saved"); },
                        Err(message) => { error.try_set(Some(message)); },
                    }
                });
            }>
                <h3>{if source.is_some() { "Organize source" } else { "New folder" }}</h3>
                {source.map(|source| view! { <p>{source.display_name}</p> })}
                <Show when=move || matches!(dialog, Dialog::NewFolder)>
                    <p>"Name the folder and choose its first source."</p>
                    <label for="folder-first-source">"Source"</label>
                    <select id="folder-first-source" disabled=move || busy.get() prop:value=move || selected_url.get() on:change=move |event| selected_url.set(event_target_value(&event))>
                        <option value="">"Choose a source"</option>
                        {move || organization.feeds.get().into_iter().map(|feed| view! { <option value=feed.url>{feed.display_name}</option> }).collect_view()}
                    </select>
                </Show>
                <Show when=move || creating.get() fallback=move || view! {
                    <label for="source-destination-folder">"Folder"</label>
                    <select id="source-destination-folder" disabled=move || busy.get() prop:value=move || selected_folder.get() on:change=move |event| selected_folder.set(event_target_value(&event))>
                        <option value="">"Unfiled"</option>
                        {move || folders.get().into_iter().map(|folder| { let value = folder.clone(); view! { <option value=value>{folder}</option> } }).collect_view()}
                    </select>
                    <button type="button" class="new-folder-choice" disabled=move || busy.get() on:click=move |_| creating.set(true)>"New folder…"</button>
                }>
                    <label for="source-new-folder">"Folder name"</label>
                    <input id="source-new-folder" type="text" maxlength="4096" autofocus=true disabled=move || busy.get() prop:value=move || new_name.get() on:input=move |event| new_name.set(event_target_value(&event))/>
                </Show>
                {move || error.get().map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
                <div class="feed-dialog-actions">
                    <button type="button" on:click=move |_| close.run(())>"Cancel"</button>
                    <button type="submit" disabled=move || busy.get()>{move || if creating.get() { "Create folder" } else { "Move source" }}</button>
                </div>
            </form>
            {remove.map(|source| view! { <button class="source-remove" type="button" disabled=move || busy.get() on:click=move |_| organization.controller.remove_target.set(Some(source.clone()))>"Remove source…"</button> })}
        </Modal>
    }
}

fn focus_source(_url: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let href = feeds::feed_href(_url);
        set_timeout(
            move || {
                let Some(document) = web_sys::window().and_then(|window| window.document()) else {
                    return;
                };
                if document
                    .query_selector("dialog[open]")
                    .ok()
                    .flatten()
                    .is_some()
                {
                    return;
                }
                let target = document
                    .query_selector(&format!(".feed-nav-row a[href=\"{href}\"]"))
                    .ok()
                    .flatten()
                    .and_then(|element| {
                        if element.closest("[hidden]").ok().flatten().is_some() {
                            element
                                .closest(".source-group")
                                .ok()
                                .flatten()
                                .and_then(|group| {
                                    group
                                        .query_selector(".source-group-heading a")
                                        .ok()
                                        .flatten()
                                })
                        } else {
                            Some(element)
                        }
                    })
                    .or_else(|| {
                        document
                            .query_selector(
                                ".sidebar-section-actions button[aria-label=\"Add source\"]",
                            )
                            .ok()
                            .flatten()
                    });
                if let Some(element) =
                    target.and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    let options = web_sys::FocusOptions::new();
                    options.set_prevent_scroll(true);
                    let _ = element.focus_with_options(&options);
                }
            },
            std::time::Duration::ZERO,
        );
    }
}

fn spawn(future: impl Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
    #[cfg(not(target_arch = "wasm32"))]
    drop(future);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(url: &str, folder: Option<&str>) -> FeedSummary {
        FeedSummary {
            url: url.into(),
            display_name: "Example".into(),
            title: None,
            description: None,
            folder: folder.map(str::to_string),
            last_fetched_label: "Never".into(),
            fetch_interval_seconds: 3600,
            consecutive_failures: 0,
            auto_mark_read: false,
        }
    }

    #[test]
    fn pending_moves_retain_source_order_metadata_and_the_native_snapshot() {
        let base = vec![source("a", None), source("b", Some("Essays"))];
        let pending = BTreeMap::from([
            ("a".into(), Some("Research / 日本語".into())),
            ("b".into(), None),
        ]);
        let result = project(base.clone(), &pending);
        assert_eq!(result[0].folder.as_deref(), Some("Research / 日本語"));
        assert_eq!(result[1].folder, None);
        assert_eq!(
            result[0],
            FeedSummary {
                folder: Some("Research / 日本語".into()),
                ..base[0].clone()
            }
        );
        assert_eq!(
            result[1],
            FeedSummary {
                folder: None,
                ..base[1].clone()
            }
        );
        assert_eq!(base[0].folder, None);
        assert_eq!(base[1].folder.as_deref(), Some("Essays"));
    }

    #[test]
    fn rollback_reveals_new_sync_state_and_never_resurrects_a_removed_source() {
        let latest = vec![source("a", Some("Arrived from another desktop"))];
        let pending = BTreeMap::from([
            ("a".into(), Some("Pending".into())),
            ("removed".into(), None),
        ]);
        assert_eq!(project(latest.clone(), &pending).len(), 1);
        assert_eq!(project(latest.clone(), &BTreeMap::new()), latest);
        assert!(project(Vec::new(), &pending).is_empty());
    }
}
