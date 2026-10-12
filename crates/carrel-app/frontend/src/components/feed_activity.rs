//! Deliberate, source-scoped recovery without interrupting the reading loop.

use crate::{components::Modal, feeds};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub(crate) fn FeedActivity() -> impl IntoView {
    let controller = feeds::use_feed_controller();
    view! { <Show when=move || controller.activity_open.get()><ActivityDialog/></Show> }
}

#[component]
fn ActivityDialog() -> impl IntoView {
    let controller = feeds::use_feed_controller();
    view! {
        <Modal label="Source activity" class="feed-dialog source-activity" on_cancel=UnsyncCallback::new(move |()| controller.activity_open.set(false))>
            <h3>"Source activity"</h3>
            <SourceActivityContent on_open=UnsyncCallback::new(move |()| controller.activity_open.set(false))/>
            <div class="feed-dialog-actions"><button type="button" on:click=move |_| controller.activity_open.set(false)>"Done"</button></div>
        </Modal>
    }
}

#[component]
pub(crate) fn SourceActivityContent(
    on_open: UnsyncCallback<()>,
    #[prop(default = false)] manage: bool,
) -> impl IntoView {
    let controller = feeds::use_feed_controller();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    view! {
        <div class="source-activity">
            <p class="source-activity-intro">"Saved articles stay available when a source cannot be reached. Retry a source to recover missing content."</p>
            <p class="source-activity-status" role="status">{move || controller.status().get().message}</p>
            <Show when=move || manage><datalist id="source-folders">{move || controller.feeds().with(|feeds| feeds.iter().filter_map(|feed| feed.folder.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().map(|folder| view! { <option value=folder/> }).collect_view())}</datalist></Show>
            <ul>
                <For each=move || controller.feeds().get() key=|feed| feed.url.clone() let:feed>
                    {
                        let url = feed.url.clone();
                        let retry_url = feed.url.clone();
                        let remove_button = NodeRef::<leptos::html::Button>::new();
                        let href = feeds::feed_href(&feed.url);
                        let state = Memo::new(move |_| controller.feeds().with(|feeds| feeds.iter().find(|feed| feed.url == url).cloned()));
                        let url = feed.url.clone();
                        let issues = Memo::new(move |_| controller.status().with(|status| status.issues.iter().filter(|issue| issue.feed_url == url).cloned().collect::<Vec<_>>()));
                        view! {
                            <li>
                                <div class="source-activity-heading">
                                    <A href=href on:click=move |_| on_open.run(())>{move || state.get().map(|feed| feed.display_name).unwrap_or_default()}</A>
                                    <Show when=move || manage><button node_ref=remove_button type="button" on:click=move |_| {
                                        if let Some(button) = remove_button.get_untracked() { let _ = button.focus(); }
                                        controller.remove_target.set(state.get_untracked());
                                    }>"Remove"</button></Show>
                                    <button type="button" disabled=move || busy.get() || controller.manual_fetch_busy.get() || controller.status().get().is_fetching on:click=move |_| {
                                        busy.set(true); error.set(None);
                                        let url = retry_url.clone();
                                        #[cfg(target_arch = "wasm32")]
                                        wasm_bindgen_futures::spawn_local(async move {
                                            if let Err(message) = feeds::fetch_one(controller, url).await { error.try_set(Some(message)); }
                                            busy.try_set(false);
                                        });
                                        #[cfg(not(target_arch = "wasm32"))]
                                        let _ = url;
                                    }>{move || if issues.get().is_empty() { "Refresh source" } else { "Retry source" }}</button>
                                </div>
                                <Show when=move || manage><SourceFolderEditor state=state/></Show>
                                <p class="source-activity-date">{move || state.get().map(|feed| if feed.consecutive_failures > 0 && issues.get().is_empty() { "Last update failed; retry this source.".into() } else { format!("Last checked {}", feed.last_fetched_label) }).unwrap_or_default()}</p>
                                {move || issues.get().into_iter().map(|issue| view! {
                                    <div class="source-problem"><p>{issue.message}</p><details><summary>"Technical details"</summary><p>{issue.detail}</p></details></div>
                                }).collect_view()}
                            </li>
                        }
                    }
                </For>
            </ul>
            <Show when=move || controller.feeds().get().is_empty()><p>"No sources yet. Add a source from Sources or import OPML."</p></Show>
            {move || error.get().map(|message| view! { <div class="source-problem" role="alert"><p>"Could not retry this source. Try again when your connection is available."</p><details><summary>"Technical details"</summary><p>{message}</p></details></div> })}
        </div>
    }
}

#[component]
fn SourceFolderEditor(state: Memo<Option<crate::api::FeedSummary>>) -> impl IntoView {
    let controller = feeds::use_feed_controller();
    let draft = RwSignal::new(
        state
            .get_untracked()
            .and_then(|feed| feed.folder)
            .unwrap_or_default(),
    );
    let dirty = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        let current = state.get();
        if !dirty.get_untracked() && !busy.get_untracked() {
            draft.set(current.and_then(|feed| feed.folder).unwrap_or_default());
        }
    });
    view! {
        <form class="source-folder-editor" on:submit=move |event| {
            event.prevent_default();
            if busy.get_untracked() { return; }
            if let Some(feed) = state.get_untracked() {
                busy.set(true); error.set(None);
                let folder = draft.get_untracked();
                super::sidebar::spawn_feed_task(async move {
                    match feeds::set_folder(controller, feed.url, folder).await {
                        Ok(()) => { dirty.try_set(false); draft.try_set(state.try_get().flatten().and_then(|feed| feed.folder).unwrap_or_default()); },
                        Err(message) => { error.try_set(Some(message)); },
                    }
                    busy.try_set(false);
                });
            }
        }>
            <label><span>"Folder"</span><input type="text" list="source-folders" maxlength="4096" placeholder="Unfiled" prop:value=move || draft.get() disabled=move || busy.get() on:input=move |event| { draft.set(event_target_value(&event)); dirty.set(true); }/></label>
            <button type="submit" disabled=move || busy.get() || !dirty.get()>"Save folder"</button>
            {move || error.get().map(|message| view! { <p role="alert">{message}</p> })}
        </form>
    }
}
