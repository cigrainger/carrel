//! Explicit social commands; drafts remain local until the user shares.
use crate::{api, components::Modal, requests::Requests};
use leptos::{html, prelude::*};
use serde_json::json;

#[derive(Clone, Debug)]
pub(crate) struct ShareTarget {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Copy)]
pub(crate) struct Sharing {
    pub people: RwSignal<api::PeopleSnapshot>,
    pub error: RwSignal<Option<String>>,
    pub target: RwSignal<Option<ShareTarget>>,
    requests: StoredValue<Requests>,
}
impl Sharing {
    pub fn refresh(self) {
        let guard = self.requests.get_value();
        let request = guard.begin();
        spawn(async move {
            match api::sharing_call::<api::PeopleSnapshot>("list_people", json!({})).await {
                Ok(snapshot) if guard.accepts(request) => {
                    self.people.try_set(snapshot);
                    self.error.try_set(None);
                }
                Err(error) if guard.accepts(request) => {
                    self.error.try_set(Some(error.message().into()));
                }
                _ => {}
            }
        });
    }
    pub fn open(self, id: String, title: String) {
        self.refresh();
        self.target.set(Some(ShareTarget { id, title }));
    }
}
pub(crate) fn provide_sharing() {
    let sharing = Sharing {
        people: RwSignal::new(api::PeopleSnapshot::default()),
        error: RwSignal::new(None),
        target: RwSignal::new(None),
        requests: StoredValue::new(Requests::default()),
    };
    provide_context(sharing);
    let guard = sharing.requests.get_value();
    on_cleanup(move || guard.invalidate());
}
pub(crate) fn use_sharing() -> Sharing {
    expect_context::<Sharing>()
}

#[component]
pub(crate) fn ShareDialog() -> impl IntoView {
    let sharing = use_sharing();
    view! { <Show when=move || sharing.target.get().is_some()><ShareForm target=sharing.target.get_untracked().expect("share target")/></Show> }
}
#[component]
fn ShareForm(target: ShareTarget) -> impl IntoView {
    let sharing = use_sharing();
    let recipient = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let saved = RwSignal::new(None::<String>);
    let submit_button = NodeRef::<html::Button>::new();
    let guard = StoredValue::new(Requests::default());
    let cleanup = guard.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let target = StoredValue::new(target);
    let token = StoredValue::new(request_token());
    let cancel = UnsyncCallback::new(move |()| sharing.target.set(None));
    view! { <Modal label="Share article" class="feed-dialog share-dialog" on_cancel=cancel>
        <form on:keydown=move |event| {
            if (event.meta_key() || event.ctrl_key()) && event.key() == "Enter" {
                event.prevent_default(); event.stop_propagation();
                if let Some(button) = submit_button.get_untracked() { button.click(); }
            }
        } on:submit=move |event| {
            event.prevent_default(); if busy.get_untracked() || saved.get_untracked().is_some() { return; }
            if recipient.get_untracked().is_empty() { error.set(Some("Choose a person to share with.".into())); return; }
            let requests = guard.get_value(); let request = requests.begin(); let write = writes.get_value().begin();
            let args = json!({"request":{"requestId":token.get_value(),"itemId":target.get_value().id,"recipient":recipient.get_untracked(),"note":note.get_untracked()}});
            busy.set(true); error.set(None);
            spawn(async move {
                let result = api::sharing_call::<api::SentShareSummary>("share_article", args).await; drop(write); sharing.refresh();
                if !requests.accepts(request) { return; }
                match result { Ok(share) => { saved.try_set(Some(share.to)); }, Err(problem) => { error.try_set(Some(problem.message().into())); } }
                busy.try_set(false);
            });
        }>
            <h3>"Share article"</h3><p class="share-title">{target.get_value().title}</p>
            <Show when=move || saved.get().is_none() fallback=move || view! { <p role="status">{move || format!("Share saved for {}. It will be delivered when your desktops can connect.",saved.get().unwrap_or_default())}</p><div class="feed-dialog-actions"><button type="button" on:click=move |_| cancel.run(())>"Done"</button></div> }>
                <label for="share-person">"Share with"</label>
                <select id="share-person" autofocus=true disabled=move || busy.get() prop:value=move || recipient.get() on:change=move |event| recipient.set(event_target_value(&event))>
                    <option value="">"Choose a person"</option>
                    {move || sharing.people.get().people.into_iter().filter(|person| person.followed).map(|person| view! { <option value=person.key>{person.name}</option> }).collect_view()}
                </select>
                <Show when=move || !sharing.people.get().people.iter().any(|person| person.followed)><p>"Add a trusted person in Settings → People, then return to this article."</p></Show>
                <label for="share-note">"A note (optional)"</label>
                <textarea id="share-note" rows="5" maxlength="65536" readonly=move || busy.get() prop:value=move || note.get() on:input=move |event| note.set(event_target_value(&event))/>
                <p class="capture-notice">"Shares the article and this note with the person you choose. Your private highlights and notes stay private."</p>
                {move || error.get().or_else(|| sharing.error.get()).map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
                <div class="feed-dialog-actions"><button type="button" on:click=move |_| cancel.run(())>"Cancel"</button><button node_ref=submit_button type="submit" disabled=move || busy.get() || recipient.get().is_empty()>"Share article"</button></div>
            </Show>
        </form>
    </Modal> }
}

#[component]
pub(crate) fn PeoplePanel() -> impl IntoView {
    let sharing = use_sharing();
    sharing.refresh();
    #[cfg(target_arch = "wasm32")]
    if let Ok(interval) =
        set_interval_with_handle(move || sharing.refresh(), std::time::Duration::from_secs(3))
    {
        on_cleanup(move || interval.clear());
    }
    let contact = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let preview = RwSignal::new(None::<api::ContactPreview>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let code = NodeRef::<html::Textarea>::new();
    let key = NodeRef::<html::Input>::new();
    let change = move |command: &'static str, args: serde_json::Value| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        spawn(async move {
            let result = api::sharing_call::<()>(command, args).await;
            drop(write);
            sharing.refresh();
            if !guard.accepts(request) {
                return;
            }
            match result {
                Ok(()) => {
                    if command == "follow_person" {
                        contact.try_set(String::new());
                        name.try_set(String::new());
                        preview.try_set(None);
                    }
                }
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                }
            }
            busy.try_set(false);
        });
    };
    view! { <section class="people-panel"><h3>"People"</h3><p>"Choose the people whose reading you trust. Exchange contact codes directly and check their public key before following."</p>
        {move || error.get().or_else(|| sharing.error.get()).map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
        <Show when=move || sharing.people.get().key.is_some() fallback=|| view! { <p>"Set up your identity in Settings → Devices first."</p> }>
            <label for="social-key">"Your public key — select and copy"</label><input node_ref=key id="social-key" readonly=true prop:value=move || sharing.people.get().key.unwrap_or_default() on:focus=move |_| { if let Some(input) = key.get() { input.select(); } }/>
            <Show when=move || sharing.people.get().ready fallback=|| view! { <p>"Your contact code is preparing. Reading remains available."</p> }>
                <label for="social-code">"Your contact code — select and copy"</label><textarea node_ref=code id="social-code" rows="3" readonly=true prop:value=move || sharing.people.get().contact_code.unwrap_or_default() on:focus=move |_| { if let Some(area) = code.get() { area.select(); } }/>
                <p class="capture-notice">"This code identifies this desktop and its connection address. Send it only to a person you want to connect with."</p>
            </Show>
            <form class="follow-person" on:submit=move |event| {
                event.prevent_default(); if busy.get_untracked() { return; }
                if preview.get_untracked().is_some() { change("follow_person",json!({"contact":contact.get_untracked(),"name":name.get_untracked()})); return; }
                busy.set(true); error.set(None); let guard = requests.get_value(); let request = guard.begin(); let text = contact.get_untracked();
                spawn(async move {
                    let result = api::sharing_call::<api::ContactPreview>("inspect_person",json!({"contact":text})).await;
                    if !guard.accepts(request) { return; }
                    match result { Ok(person) => { preview.try_set(Some(person)); }, Err(problem) => { error.try_set(Some(problem.message().into())); } }
                    busy.try_set(false);
                });
            }>
                <h4>"Follow a person"</h4><label for="person-code">"Public key or contact code"</label><textarea id="person-code" rows="3" maxlength="65536" readonly=move || busy.get() prop:value=move || contact.get() on:input=move |event| { contact.set(event_target_value(&event)); preview.set(None); }/>
                <label for="person-name">"Name for this person"</label><input id="person-name" maxlength="256" readonly=move || busy.get() prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event))/>
                {move || preview.get().map(|person| view! { <div class="contact-preview"><p>"Check this public key with the person:"</p><code>{person.key}</code><p>{if person.connection_info {"Verified contact code. Follow only if you recognize this key."} else {"A public key is enough to follow. Exchange a contact code to connect your desktops."}}</p></div> })}
                <button type="submit" disabled=move || busy.get() || contact.get().trim().is_empty() || name.get().trim().is_empty()>{move || if preview.get().is_some() {"Follow this person"} else {"Review public key"}}</button>
            </form>
        </Show>
        <ul class="people-list"><For each={move || sharing.people.get().people.into_iter().filter(|person| person.followed).collect::<Vec<_>>()} key=|person| (person.key.clone(), person.name.clone(), person.connected, person.error.clone()) let:person>
            <PersonRow person=person busy=busy on_remove=UnsyncCallback::new(move |key: String| change("remove_person",json!({"key":key}))) on_retry=UnsyncCallback::new(move |()| change("refresh_shares",json!({})))/>
        </For></ul>
        <h4>"Sent articles"</h4><p>"Withdrawing removes your vouch. A person may keep an article they already received."</p>
        <ul class="sent-shares"><For each=move || sharing.people.get().shares key=|share| (share.id.clone(),share.retracted) let:share>
            <SentRow share=share busy=busy on_retract=UnsyncCallback::new(move |id: String| change("retract_share",json!({"id":id})))/>
        </For></ul>
    </section> }
}
#[component]
fn PersonRow(
    person: api::PersonSummary,
    busy: RwSignal<bool>,
    on_remove: UnsyncCallback<String>,
    on_retry: UnsyncCallback<()>,
) -> impl IntoView {
    let confirm = RwSignal::new(false);
    let key = StoredValue::new(person.key);
    view! { <li><strong>{person.name}</strong><code>{key.get_value()}</code><p>{if person.connected {"Contact code available"} else {"Waiting for a contact code"}}</p>{person.error.map(|message| view! { <p class="feed-error">{message}</p> })}
        <Show when=move || confirm.get() fallback=move || view! { <button type="button" disabled=move || busy.get() on:click=move |_| confirm.set(true)>"Remove person"</button> }><p>"Stop exchanging articles with this person? Articles already received stay in Library."</p><button type="button" on:click=move |_| confirm.set(false)>"Cancel"</button><button type="button" disabled=move || busy.get() on:click=move |_| on_remove.run(key.get_value())>"Remove person"</button></Show>
        <button type="button" disabled=move || busy.get() on:click=move |_| on_retry.run(())>"Try connection again"</button>
    </li> }
}
#[component]
fn SentRow(
    share: api::SentShareSummary,
    busy: RwSignal<bool>,
    on_retract: UnsyncCallback<String>,
) -> impl IntoView {
    let confirm = RwSignal::new(false);
    let id = StoredValue::new(share.id);
    let preferences = crate::preferences::use_preferences();
    view! { <li><p><a href=format!("/item/{}",share.item_id) on:click=move |_| preferences.settings_open.set(false)>{share.title}</a></p><p>{format!("To {} · {}",share.to,share.date)}</p>{share.note.map(|note| view! { <blockquote>{note}</blockquote> })}
        <Show when=move || !share.retracted fallback=|| view! { <p>"Withdrawn"</p> }>
            <Show when=move || confirm.get() fallback=move || view! { <button type="button" disabled=move || busy.get() on:click=move |_| confirm.set(true)>"Withdraw share"</button> }><p>"Withdraw this vouch?"</p><button type="button" on:click=move |_| confirm.set(false)>"Cancel"</button><button type="button" disabled=move || busy.get() on:click=move |_| on_retract.run(id.get_value())>"Withdraw share"</button></Show>
        </Show>
    </li> }
}
#[cfg(target_arch = "wasm32")]
fn request_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "ui-{}-{}",
        web_sys::js_sys::Date::now() as u64,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
#[cfg(not(target_arch = "wasm32"))]
fn request_token() -> String {
    "ssr-preview".into()
}
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl std::future::Future<Output = ()> + 'static) {
    leptos::task::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(_task: impl std::future::Future<Output = ()> + 'static) {}
