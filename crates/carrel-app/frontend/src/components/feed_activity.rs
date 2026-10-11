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
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    view! {
        <Modal label="Source activity" class="feed-dialog source-activity" on_cancel=UnsyncCallback::new(move |()| controller.activity_open.set(false))>
            <h3>"Source activity"</h3>
            <p class="source-activity-intro">"Saved articles stay available when a source cannot be reached. Retry a source to recover missing content."</p>
            <p class="source-activity-status" role="status">{move || controller.status().get().message}</p>
            <ul>
                <For each=move || controller.feeds().get() key=|feed| feed.url.clone() let:feed>
                    {
                        let url = feed.url.clone();
                        let retry_url = feed.url.clone();
                        let href = feeds::feed_href(&feed.url);
                        let state = Memo::new(move |_| controller.feeds().with(|feeds| feeds.iter().find(|feed| feed.url == url).cloned()));
                        let url = feed.url.clone();
                        let issues = Memo::new(move |_| controller.status().with(|status| status.issues.iter().filter(|issue| issue.feed_url == url).cloned().collect::<Vec<_>>()));
                        view! {
                            <li>
                                <div class="source-activity-heading">
                                    <A href=href on:click=move |_| controller.activity_open.set(false)>{move || state.get().map(|feed| feed.display_name).unwrap_or_default()}</A>
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
            <div class="feed-dialog-actions"><button type="button" on:click=move |_| controller.activity_open.set(false)>"Done"</button></div>
        </Modal>
    }
}
