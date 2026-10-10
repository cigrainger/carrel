//! Quiet, explicit desktop pairing alongside subscription management.

use leptos::prelude::*;
use serde_json::json;

use crate::{api, components::Modal, feeds};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Overview,
    Setup,
    Join,
    Invite,
    Revoke,
}

#[component]
pub(crate) fn Devices() -> impl IntoView {
    let open = RwSignal::new(false);
    let mode = RwSignal::new(Mode::Overview);
    let snapshot = RwSignal::new(api::SyncSnapshot::default());
    let name = RwSignal::new(String::new());
    let passphrase = RwSignal::new(String::new());
    let confirm = RwSignal::new(String::new());
    let invitation = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let selected_device = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let feeds = feeds::use_feed_controller();
    let seen_revision = RwSignal::new(0u64);

    let poll = move || {
        leptos::task::spawn_local(async move {
            if let Ok(current) = api::sync_status().await {
                if current.revision != seen_revision.get_untracked() {
                    seen_revision.set(current.revision);
                    feeds.sync_revision.set(current.revision);
                    feeds::refresh_from_sync(feeds).await;
                }
                snapshot.set(current);
            }
        });
    };
    #[cfg(target_arch = "wasm32")]
    {
        poll();
        if let Ok(interval) = set_interval_with_handle(poll, std::time::Duration::from_secs(3)) {
            on_cleanup(move || interval.clear());
        }
    }
    let dismiss = move |()| {
        open.set(false);
        passphrase.set(String::new());
        confirm.set(String::new());
        invitation.set(String::new());
        error.set(None);
        leptos::task::spawn_local(async move {
            let _ = api::sync_action("cancel_sync_invitation", json!({})).await;
        });
    };
    let submit = move |command: &'static str, args: serde_json::Value| {
        busy.set(true);
        error.set(None);
        passphrase.set(String::new());
        confirm.set(String::new());
        leptos::task::spawn_local(async move {
            match api::sync_action(command, args).await {
                Ok(text) => {
                    if let Some(text) = text {
                        invitation.set(text);
                    } else {
                        invitation.set(String::new());
                        mode.set(Mode::Overview);
                    }
                    poll();
                    feeds::refresh_from_sync(feeds).await;
                }
                Err(problem) => error.set(Some(problem.message().into())),
            }
            busy.set(false);
        });
    };

    view! {
        <button class="devices-link" type="button" on:click=move |_| {mode.set(Mode::Overview); open.set(true); poll();}>"Devices"</button>
        <Show when=move || open.get()>
            <Modal label="Your desktops" class="feed-dialog devices-dialog" on_cancel=UnsyncCallback::new(dismiss)>
                <div class="dialog-heading"><h2>"Your desktops"</h2><button type="button" aria-label="Close devices" on:click=move |_| dismiss(())>"Close"</button></div>
                <p>"Keep your reading library on your own desktops. Changes wait locally while a desktop is offline."</p>
                {move || error.get().or_else(|| snapshot.get().error).map(|text| view! {<p class="feed-error" role="alert">{text}</p>})}
                <Show when=move || mode.get() == Mode::Overview>
                    <Show when=move || snapshot.get().enabled fallback=move || view! {
                        <div class="dialog-actions">
                            <button type="button" on:click=move |_| mode.set(Mode::Setup)>"Set up sync"</button>
                            <button type="button" on:click=move |_| mode.set(Mode::Join)>"Use an invitation"</button>
                        </div>
                    }>
                        <p class="sync-state">{move || if snapshot.get().active {"Sync is available when your desktops can connect."} else {"Sync is preparing. Reading remains available."}}</p>
                        <ul class="device-list">
                            <For each=move || snapshot.get().devices key=|device| device.key.clone() let:device>
                                {
                                    let removable = !device.local && !device.revoked;
                                    let key = StoredValue::new(device.key.clone());
                                    view! { <li><span>{device.name}{if device.local {" · this desktop"} else {""}}{if device.revoked {" · removed"} else {""}}</span>
                                        <Show when=move || removable && snapshot.get().can_invite>
                                            <button type="button" on:click=move |_| { selected_device.set(key.get_value()); mode.set(Mode::Revoke); }>"Remove"</button>
                                        </Show>
                                    </li> }
                                }
                            </For>
                        </ul>
                        <Show when=move || snapshot.get().can_invite>
                            <button type="button" on:click=move |_| mode.set(Mode::Invite)>"Pair another desktop"</button>
                        </Show>
                        <Show when=move || !snapshot.get().can_invite><p>"Create invitations from the desktop holding your recovery key."</p></Show>
                    </Show>
                </Show>
                <Show when=move || mode.get() != Mode::Overview>
                    <form on:submit=move |event| {
                        event.prevent_default();
                        let current = mode.get_untracked();
                        if current == Mode::Setup && passphrase.get_untracked() != confirm.get_untracked() { error.set(Some("The passphrases do not match.".into())); return; }
                        match current {
                            Mode::Setup => submit("enable_sync", json!({"passphrase":passphrase.get_untracked(), "name":name.get_untracked()})),
                            Mode::Join => submit("join_sync_invitation", json!({"invitation":invitation.get_untracked(), "name":name.get_untracked()})),
                            Mode::Invite => submit("create_sync_invitation", json!({"passphrase":passphrase.get_untracked()})),
                            Mode::Revoke => submit("revoke_sync_device", json!({"passphrase":passphrase.get_untracked(), "key":selected_device.get_untracked()})),
                            Mode::Overview => {},
                        }
                    }>
                        <Show when=move || matches!(mode.get(), Mode::Setup | Mode::Join)>
                            <label>"Desktop name"<input type="text" required maxlength="256" autocomplete="off" prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() != Mode::Join && invitation.get().is_empty()>
                            <label>"Recovery passphrase"<input type="password" required minlength="8" autocomplete="off" prop:value=move || passphrase.get() on:input=move |event| passphrase.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Setup>
                            <p>"This passphrase protects the recovery key stored on this desktop. Ordinary reading and sync do not require it."</p>
                            <label>"Repeat passphrase"<input type="password" required minlength="8" autocomplete="off" prop:value=move || confirm.get() on:input=move |event| confirm.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Join || !invitation.get().is_empty()>
                            <label>{move || if mode.get() == Mode::Join {"Paste invitation"} else {"Copy this invitation to your other desktop"}}
                                <textarea required rows="5" readonly=move || mode.get() == Mode::Invite prop:value=move || invitation.get() on:input=move |event| invitation.set(event_target_value(&event))/>
                            </label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Invite && !invitation.get().is_empty()>
                            <p>"Valid for five minutes and one desktop. Keep this window open while pairing."</p>
                        </Show>
                        <div class="dialog-actions">
                            <button type="button" disabled=move || busy.get() on:click=move |_| { mode.set(Mode::Overview); invitation.set(String::new()); passphrase.set(String::new()); error.set(None); leptos::task::spawn_local(async move { let _ = api::sync_action("cancel_sync_invitation", json!({})).await; }); }>"Back"</button>
                            <Show when=move || mode.get() != Mode::Invite || invitation.get().is_empty()>
                                <button type="submit" disabled=move || busy.get()>{move || if busy.get() {"Working…"} else {match mode.get() {Mode::Setup=>"Enable sync", Mode::Join=>"Pair desktop", Mode::Revoke=>"Remove desktop", _=>"Create invitation"}}}</button>
                            </Show>
                        </div>
                    </form>
                </Show>
            </Modal>
        </Show>
    }
}
