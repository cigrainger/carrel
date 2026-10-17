//! Article links remain useful before a reader has connected with anyone.

use carrel_core::app_link::AppLink;
use leptos::{html, prelude::*};

use super::{Icon, IconName, status_strip::PopoverDismissal};
use crate::{api::ItemDetail, app_links::AppLinksController, sharing::Sharing};

#[component]
pub(crate) fn ShareMenu(
    #[prop(into)] item: Signal<Option<ItemDetail>>,
    on_share: UnsyncCallback<()>,
) -> impl IntoView {
    let sharing = use_context::<Sharing>();
    let links = use_context::<AppLinksController>();
    let preferences = use_context::<crate::preferences::PreferencesController>();
    let menu = NodeRef::<html::Details>::new();
    let trigger = NodeRef::<html::Summary>::new();
    let open = RwSignal::new(false);
    let can_share = Signal::derive(move || {
        sharing.is_some_and(|sharing| {
            sharing.loaded.get()
                && sharing
                    .people
                    .get()
                    .people
                    .iter()
                    .any(|person| person.followed)
        })
    });
    let close = UnsyncCallback::new(move |restore: bool| {
        if let Some(menu) = menu.get_untracked() {
            let _ = menu.remove_attribute("open");
        }
        open.set(false);
        if restore && let Some(trigger) = trigger.get_untracked() {
            let _ = trigger.focus();
        }
    });
    view! {
        <details class="share-menu" node_ref=menu on:toggle=move |_| {
            let expanded = menu.get_untracked().is_some_and(|menu| menu.has_attribute("open"));
            open.set(expanded);
            if expanded && let Some(sharing) = sharing { sharing.refresh(); }
        }>
            <summary node_ref=trigger aria-label="Share article" title="Sharing and links"
                aria-disabled=move || item.get().is_none().to_string()
                tabindex=move || if item.get().is_some() { "0" } else { "-1" }
                aria-expanded=move || open.get().to_string()
                on:click=move |event| { if item.get_untracked().is_none() { event.prevent_default(); } }><Icon name=IconName::Share/></summary>
            <Show when=move || open.get()>
                <PopoverDismissal name="share-menu" menu=menu on_close=close/>
                <div class="share-menu-panel" aria-label="Sharing and links">
                    <button type="button" disabled=move || links.is_none() || item.get().is_none_or(|item| item.primary_url.is_none())
                        on:click=move |_| {
                            close.run(true);
                            if let (Some(links), Some(url)) = (links, item.get_untracked().and_then(|item| item.primary_url)) { links.copy_original(url); }
                        }>"Copy link"</button>
                    <button type="button" disabled=move || links.is_none() || item.get().is_none()
                        on:click=move |_| {
                            close.run(true);
                            if let (Some(links), Some(link)) = (links, item.get_untracked().and_then(|item| AppLink::item(&item.id).ok())) { links.copy(link); }
                        }>"Copy Carrel link"</button>
                    <p>"Carrel links open saved content; they do not grant access."</p>
                    <hr/>
                    <button type="button" on:click=move |_| {
                        close.run(true);
                        if can_share.get_untracked() { on_share.run(()); }
                        else if let Some(preferences) = preferences { preferences.open_settings(crate::preferences::SettingsSection::People); }
                    }>{move || if can_share.get() { "Share with people…" } else { "Add a person…" }}</button>
                    <Show when=move || sharing.is_some_and(|sharing| sharing.loading.get())><p role="status">"Checking people…"</p></Show>
                    <Show when=move || sharing.is_some_and(|sharing| sharing.error.get().is_some())>
                        <p>"Could not check your people."</p><button type="button" on:click=move |_| { if let Some(sharing) = sharing { sharing.refresh(); } }>"Retry people"</button>
                    </Show>
                </div>
            </Show>
        </details>
    }
}
