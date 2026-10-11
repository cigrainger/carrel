//! Deliberate URL capture, without disturbing the current reader or list.

use leptos::{html, prelude::*};
use leptos_router::{components::A, hooks::use_navigate};

use crate::{
    api,
    components::{Icon, IconName, Modal},
    navigation::ListNavigation,
};

/// Session visibility of the explicit capture dialog.
#[derive(Clone, Copy)]
pub(crate) struct CaptureDialog(pub RwSignal<bool>);

impl CaptureDialog {
    /// WebKit does not focus buttons on pointer activation. Focus the initiator
    /// before opening so the native dialog restores it on dismissal.
    pub(crate) fn open_from(self, initiator: NodeRef<html::Button>) {
        #[cfg(target_arch = "wasm32")]
        if let Some(button) = initiator.get_untracked() {
            let _ = button.focus();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = initiator;
        self.0.set(true);
    }
}

pub(crate) fn provide_capture_dialog() {
    provide_context(CaptureDialog(RwSignal::new(false)));
}

pub(crate) fn use_capture_dialog() -> CaptureDialog {
    use_context::<CaptureDialog>().expect("Capture dialog context")
}

/// Mount the capture dialog outside the current article/list route lifetime.
#[component]
pub(crate) fn ArticleCapture() -> impl IntoView {
    let open = use_capture_dialog().0;
    view! { <Show when=move || open.get()><SaveArticle/></Show> }
}

#[component]
fn SaveArticle() -> impl IntoView {
    let open = use_capture_dialog().0;
    let input = NodeRef::<html::Input>::new();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let saved = RwSignal::new(None::<api::SavedArticle>);
    let read_button = NodeRef::<html::Button>::new();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if saved.get().is_some()
            && let Some(button) = read_button.get()
        {
            let _ = button.focus();
        }
    });
    let requests = crate::requests::Requests::default();
    let cleanup = requests.clone();
    on_cleanup(move || cleanup.invalidate());
    let status = crate::status::use_status_controller();
    let feeds = crate::feeds::use_feed_controller();
    let writes = crate::writes::use_pending_writes();
    let navigate = use_navigate();
    let navigation = crate::navigation::use_list_navigation_context();
    let cancel = UnsyncCallback::new(move |()| open.set(false));
    view! {
        <Modal label="Save article" class="feed-dialog capture-dialog" on_cancel=cancel>
            <form on:submit=move |event| {
                event.prevent_default();
                if busy.get_untracked() || saved.get_untracked().is_some() { return; }
                let url = input.get_untracked().map(|input| input.value()).unwrap_or_default().trim().to_string();
                if url.is_empty() { error.set(Some("Enter an article address".into())); return; }
                let guard = requests.clone();
                let request = guard.begin();
                let write = writes.begin();
                busy.set(true);
                error.set(None);
                spawn_capture(async move {
                    let result = api::save_article(url).await;
                    drop(write);
                    match result {
                        Ok(article) => {
                            feeds.bump_refresh();
                            if let Some(warning) = &article.warning { status.warn(warning.clone()); }
                            else { status.info(if article.existed { "Article already in Library" } else { "Article saved to Library" }); }
                            if guard.accepts(request) { saved.try_set(Some(article)); }
                        }
                        Err(failure) => {
                            if guard.accepts(request) { error.try_set(Some(failure.message().into())); }
                            else { status.warn(format!("Article could not be saved: {}", failure.message())); }
                        }
                    }
                    if guard.accepts(request) { busy.try_set(false); }
                });
            }>
                <h3>"Save article"</h3>
                <Show when=move || saved.get().is_none() fallback=move || {
                    let navigate = navigate.clone();
                    view! { <div class="capture-result" role="status">
                        <p class="capture-result-label">{move || if saved.get().is_some_and(|article| article.existed) { "Already in your library" } else { "Saved for reading" }}</p>
                        <p class="capture-result-title">{move || saved.get().map(|article| article.title).unwrap_or_default()}</p>
                        {move || saved.get().and_then(|article| article.warning).map(|message| view! { <p class="capture-notice">{message}</p> })}
                        <A href="/library" attr:class="capture-library-link" on:click=move |_| cancel.run(())>"View in Library"</A>
                        <div class="feed-dialog-actions">
                            <button type="button" on:click=move |_| cancel.run(())>"Done"</button>
                            <button node_ref=read_button type="button" on:click=move |_| {
                                if let Some(article) = saved.get_untracked() {
                                    navigation.set(ListNavigation { item_ids: vec![article.id.clone()], current_id: article.id.clone(), return_path: "/library".into(), return_label: "Library".into() });
                                    cancel.run(());
                                    navigate(&format!("/item/{}", article.id), Default::default());
                                }
                            }><Icon name=IconName::Reading/><span>"Read article"</span></button>
                        </div>
                    </div> }
                }>
                    <label for="article-url">"Article address"</label>
                    <input node_ref=input id="article-url" name="article-url" type="text" inputmode="url" autocomplete="url" placeholder="example.org/article" autofocus=true readonly=move || busy.get() aria-describedby="capture-help"/>
                    <p id="capture-help" class="capture-notice">"Keep a readable copy and images in your library."</p>
                    {move || error.get().map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
                    <div class="feed-dialog-actions">
                        <button type="button" on:click=move |_| cancel.run(())>"Cancel"</button>
                        <button type="submit" disabled=move || busy.get()>{move || if busy.get() { "Saving article…" } else { "Save article" }}</button>
                    </div>
                </Show>
            </form>
        </Modal>
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_capture(task: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn_capture(_task: impl std::future::Future<Output = ()> + 'static) {}
