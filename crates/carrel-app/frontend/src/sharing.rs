//! Explicit social commands; drafts remain local until the user shares.
use crate::components::copy_text::CopyTextField;
use crate::{api, components::Modal, requests::Requests};
use leptos::{html, prelude::*};
use serde_json::json;

#[derive(Clone, Debug)]
pub(crate) struct ShareTarget {
    pub id: String,
    pub title: String,
}

/// Session presentation state, never a sent or synchronized message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationDraft {
    pub body: String,
    pub parent: Option<String>,
    pub request_id: String,
}

#[derive(Clone, Copy)]
pub(crate) struct Sharing {
    pub people: RwSignal<api::PeopleSnapshot>,
    pub error: RwSignal<Option<String>>,
    pub target: RwSignal<Option<ShareTarget>>,
    pub conversation: RwSignal<Option<String>>,
    pub conversation_drafts: RwSignal<std::collections::BTreeMap<String, ConversationDraft>>,
    pub(crate) loading: RwSignal<bool>,
    pub(crate) loaded: RwSignal<bool>,
    history_revision: RwSignal<u64>,
    status: crate::status::StatusController,
    preferences: crate::preferences::PreferencesController,
    requests: StoredValue<Requests>,
}
impl Sharing {
    /// A late save acknowledges only its own draft, preserving newer composition.
    pub(crate) fn reply_saved(self, id: &str, submitted: &ConversationDraft) {
        self.conversation_drafts.try_update(|drafts| {
            if drafts.get(id) == Some(submitted) {
                drafts.remove(id);
            } else if let Some(current) = drafts.get_mut(id)
                && current.request_id == submitted.request_id
            {
                current.request_id = request_token();
            }
        });
    }
    /// Leaving the article supersedes a composer that has not opened yet.
    pub(crate) fn cancel_pending(self) {
        self.requests.get_value().invalidate();
        self.loading.try_set(false);
    }
    pub(crate) fn history_changed(self) {
        self.history_revision.try_update(|revision| *revision += 1);
    }
    pub fn refresh(self) {
        self.load_people(None);
    }
    fn load_people(self, target: Option<ShareTarget>) {
        let guard = self.requests.get_value();
        let request = guard.begin();
        self.loading.set(true);
        spawn(async move {
            match api::sharing_call::<api::PeopleSnapshot>("list_people", json!({})).await {
                Ok(snapshot) if guard.accepts(request) => {
                    let available = has_recipients(&snapshot);
                    self.people.try_set(snapshot);
                    self.loaded.try_set(true);
                    self.error.try_set(None);
                    if let Some(target) = target {
                        if available {
                            self.target.try_set(Some(target));
                        } else {
                            self.preferences
                                .open_settings(crate::preferences::SettingsSection::People);
                            self.status
                                .info("Add a person to share an article and note with them.");
                        }
                    }
                }
                Err(error) if guard.accepts(request) => {
                    self.error.try_set(Some(error.message().into()));
                    if target.is_some() {
                        self.status.warn("Could not check your people. Try Share again or open Settings → People.");
                    }
                }
                _ => {}
            }
            if guard.accepts(request) {
                self.loading.try_set(false);
            }
        });
    }
    pub fn open(self, id: String, title: String) {
        self.load_people(Some(ShareTarget { id, title }));
    }
}
pub(crate) fn provide_sharing() {
    let sharing = Sharing {
        people: RwSignal::new(api::PeopleSnapshot::default()),
        error: RwSignal::new(None),
        target: RwSignal::new(None),
        conversation: RwSignal::new(None),
        conversation_drafts: RwSignal::new(std::collections::BTreeMap::new()),
        loading: RwSignal::new(false),
        loaded: RwSignal::new(false),
        history_revision: RwSignal::new(0),
        status: crate::status::use_status_controller(),
        preferences: crate::preferences::use_preferences(),
        requests: StoredValue::new(Requests::default()),
    };
    provide_context(sharing);
    let guard = sharing.requests.get_value();
    on_cleanup(move || guard.invalidate());
    sharing.refresh();
}
fn has_recipients(snapshot: &api::PeopleSnapshot) -> bool {
    snapshot.people.iter().any(|person| person.followed)
}
pub(crate) fn use_sharing() -> Sharing {
    expect_context::<Sharing>()
}

#[component]
pub(crate) fn ShareDialog() -> impl IntoView {
    let sharing = use_sharing();
    view! { <Show when=move || sharing.target.get().is_some()><ShareForm target=sharing.target.get_untracked().expect("share target")/></Show><crate::conversations::ConversationDialog/> }
}
#[component]
fn ShareForm(target: ShareTarget) -> impl IntoView {
    let sharing = use_sharing();
    let recipient = RwSignal::new(String::new());
    let selected_group = RwSignal::new(None::<api::GroupSummary>);
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
            let choice=recipient.get_untracked();let group=selected_group.get_untracked();
            let args = json!({"request":{"requestId":token.get_value(),"itemId":target.get_value().id,"recipient":if group.is_none(){choice}else{String::new()},"group":group.as_ref().map(|g|g.id.clone()),"audience":group.map(|g|g.members).unwrap_or_default(),"note":note.get_untracked()}});
            busy.set(true); error.set(None);
            spawn(async move {
                let result = api::sharing_call::<api::SentShareSummary>("share_article", args).await; drop(write); sharing.refresh();
                if result.is_ok() { sharing.history_changed(); }
                if !requests.accepts(request) { return; }
                match result { Ok(share) => { saved.try_set(Some(share.to)); }, Err(problem) => { error.try_set(Some(problem.message().into())); } }
                busy.try_set(false);
            });
        }>
            <h3>"Share article"</h3><p class="share-title">{target.get_value().title}</p>
            <Show when=move || saved.get().is_none() fallback=move || view! { <p role="status">{move || format!("Share saved for {}. It will be delivered when your desktops can connect.",saved.get().unwrap_or_default())}</p><div class="feed-dialog-actions"><button type="button" on:click=move |_| cancel.run(())>"Done"</button></div> }>
                <label for="share-person">"Share with"</label>
                <select id="share-person" autofocus=true disabled=move || busy.get() prop:value=move || recipient.get() on:change=move |event| {let choice=event_target_value(&event);selected_group.set(choice.strip_prefix("group:").and_then(|id|sharing.people.get_untracked().groups.into_iter().find(|g|g.id==id)));recipient.set(choice);}>
                    <option value="">"Choose a person or group"</option>
                    {move || sharing.people.get().people.into_iter().filter(|person| person.followed).map(|person| view! { <option value=person.key>{person.name}</option> }).collect_view()}
                    {move || sharing.people.get().groups.into_iter().map(|group|view!{<option value=format!("group:{}",group.id)>{format!("Group: {}",group.name)}</option>}).collect_view()}
                </select>
                <p class="share-audience">{move || {
                    let snapshot=sharing.people.get();
                    selected_group.get().map(|g|format!("Only {}. This conversation keeps these participants even if the group changes.",g.members.iter().map(|key|snapshot.people.iter().find(|p|p.key==*key).map(|p|p.name.clone()).unwrap_or_else(||key.chars().take(12).collect())).collect::<Vec<_>>().join(", "))).unwrap_or_default()
                }}</p>
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
    let follow_input = NodeRef::<html::Textarea>::new();
    let removed = RwSignal::new(None::<String>);
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(key) = removed.get()
            && !sharing
                .people
                .get()
                .people
                .iter()
                .any(|person| person.key == key && person.followed)
        {
            removed.set(None);
            if let Some(input) = follow_input.get() {
                let options = web_sys::FocusOptions::new();
                options.set_prevent_scroll(true);
                let _ = input.focus_with_options(&options);
            }
        }
    });
    let change = move |command: &'static str, args: serde_json::Value| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        let removed_key = if command == "remove_person" {
            args.get("key")
                .and_then(|key| key.as_str())
                .map(str::to_owned)
        } else {
            None
        };
        spawn(async move {
            let result = api::sharing_call::<()>(command, args).await;
            drop(write);
            sharing.refresh();
            if result.is_ok() {
                sharing.history_changed();
            }
            if !guard.accepts(request) {
                return;
            }
            match result {
                Ok(()) => {
                    if let Some(key) = removed_key {
                        removed.try_set(Some(key));
                    }
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
        <Show when=move || sharing.people.get().key.is_some() fallback=move || view! { <p>"Create your identity before connecting with a person. You can keep reading without one."</p><button type="button" on:click=move |_| sharing.preferences.open_settings(crate::preferences::SettingsSection::Devices)>"Create identity"</button> }>
            <crate::conversations::ProfileEditor/>
            <CopyTextField id="social-key" label="Your public key" button="Copy public key" text=Signal::derive(move || sharing.people.get().key.unwrap_or_default())/>
            <Show when=move || sharing.people.get().ready fallback=|| view! { <p>"Your contact code is preparing. Reading remains available."</p> }>
                <CopyTextField id="social-code" label="Your contact code" button="Copy contact code" text=Signal::derive(move || sharing.people.get().contact_code.unwrap_or_default())/>
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
                <h4>"Follow a person"</h4><label for="person-code">"Public key or contact code"</label><textarea node_ref=follow_input id="person-code" rows="3" maxlength="65536" readonly=move || busy.get() prop:value=move || contact.get() on:input=move |event| { contact.set(event_target_value(&event)); preview.set(None); }/>
                <label for="person-name">"Name for this person"</label><input id="person-name" maxlength="256" readonly=move || busy.get() prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event))/>
                {move || preview.get().map(|person| view! { <div class="contact-preview"><p>"Check this public key with the person:"</p><code>{person.key}</code><p>{if person.connection_info {"Verified contact code. Follow only if you recognize this key."} else {"A public key is enough to follow. Exchange a contact code to connect your desktops."}}</p></div> })}
                <button type="submit" disabled=move || busy.get() || contact.get().trim().is_empty()>{move || if preview.get().is_some() {"Follow this person"} else {"Review public key"}}</button>
            </form>
        </Show>
        <p>"Each person needs to follow the other's public key. Keep both desktops open for exchange; saved shares wait while either is offline."</p>
        <ul class="people-list"><For each={move || sharing.people.get().people.into_iter().filter(|person| person.followed).collect::<Vec<_>>()} key=|person| person.key.clone() let:person>
            <PersonRow person=person busy=busy on_remove=UnsyncCallback::new(move |key: String| change("remove_person",json!({"key":key}))) on_retry=UnsyncCallback::new(move |()| change("refresh_shares",json!({})))/>
        </For></ul>
        <Show when=move || sharing.people.get().key.is_some()><crate::conversations::GroupsPanel/></Show>
        <h4>"Sent articles"</h4><p>"Withdrawing removes your vouch. A person may keep an article they already received."</p>
        <SentHistory busy=busy on_retract=UnsyncCallback::new(move |id: String| change("retract_share",json!({"id":id})))/>
    </section> }
}
#[component]
fn SentHistory(busy: RwSignal<bool>, on_retract: UnsyncCallback<String>) -> impl IntoView {
    let sharing = use_sharing();
    let shares = RwSignal::new(Vec::<api::SentShareSummary>::new());
    let next = RwSignal::new(None::<api::SentShareCursor>);
    let pages = RwSignal::new(1_usize);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let retry_older = RwSignal::new(false);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let load = UnsyncCallback::new(move |older: bool| {
        if older && loading.get_untracked() {
            return;
        }
        let guard = requests.get_value();
        let request = guard.begin();
        let wanted = pages.get_untracked();
        let after = if older { next.get_untracked() } else { None };
        let mut result = if older {
            shares.get_untracked()
        } else {
            Vec::new()
        };
        loading.set(true);
        error.set(None);
        retry_older.set(older);
        spawn(async move {
            let mut cursor = after;
            for _ in 0..if older { 1 } else { wanted } {
                match api::sharing_call::<api::SentSharePage>(
                    "list_sent_shares",
                    json!({"after":cursor}),
                )
                .await
                {
                    Ok(page) => {
                        if !guard.accepts(request) {
                            return;
                        }
                        for share in page.shares {
                            if !result.iter().any(|known| known.id == share.id) {
                                result.push(share);
                            }
                        }
                        cursor = page.next_cursor;
                        if cursor.is_none() {
                            break;
                        }
                    }
                    Err(problem) => {
                        if guard.accepts(request) {
                            error.try_set(Some(problem.message().into()));
                            loading.try_set(false);
                        }
                        return;
                    }
                }
            }
            if guard.accepts(request) {
                shares.try_set(result);
                next.try_set(cursor);
                if older {
                    pages.try_update(|pages| *pages += 1);
                }
                loading.try_set(false);
            }
        });
    });
    Effect::new(move |_| {
        sharing.history_revision.get();
        load.run(false);
    });
    view! { <div class="sent-history">{move ||error.get().map(|message|view!{<p class="feed-error" role="alert">{message}</p><button type="button" disabled=move ||loading.get() on:click=move |_|load.run(retry_older.get_untracked())>"Retry sent articles"</button>})}
        <ul class="sent-shares"><For each=move ||shares.get() key=|share|share.id.clone() let:share>{
            let id=StoredValue::new(share.id.clone());let fallback=StoredValue::new(share);
            view!{<SentRow share=Signal::derive(move ||shares.with(|list| list.iter().find(|share|share.id==id.get_value()).cloned()).unwrap_or_else(||fallback.get_value())) busy=Signal::derive(move ||busy.get() || loading.get()) on_retract=on_retract/>}
        }</For></ul>
        <Show when=move ||next.get().is_some()><button type="button" disabled=move ||loading.get() || busy.get() on:click=move |_|load.run(true)>"Load older sent articles"</button></Show>
    </div> }
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
    let sharing = use_sharing();
    let person = Memo::new(move |_| {
        sharing
            .people
            .get()
            .people
            .into_iter()
            .find(|person| person.key == key.get_value())
    });
    view! { <li><crate::conversations::Avatar name=Signal::derive(move ||person.get().map(|p|p.name).unwrap_or_default()) image=Signal::derive(move ||person.get().and_then(|p|p.image))/><strong>{move || person.get().map(|person|person.name).unwrap_or_default()}</strong><code>{key.get_value()}</code><p>{move || if person.get().is_some_and(|person|person.connected) {"Contact code available"} else {"Waiting for a contact code. Ask this person for their current code and review it above using the same key."}}</p>{move || person.get().and_then(|person|person.error).map(|message| view! { <p class="feed-error">{message}</p> })}
        <Show when=move || confirm.get() fallback=move || view! { <button type="button" disabled=move || busy.get() on:click=move |_| confirm.set(true)>"Remove person"</button> }><p>"Stop exchanging articles with this person? Articles already received stay in Library."</p><button type="button" on:click=move |_| confirm.set(false)>"Cancel"</button><button type="button" disabled=move || busy.get() on:click=move |_| on_remove.run(key.get_value())>"Remove person"</button></Show>
        <button type="button" disabled=move || busy.get() on:click=move |_| on_retry.run(())>"Try connection again"</button>
    </li> }
}
#[component]
fn SentRow(
    #[prop(into)] share: Signal<api::SentShareSummary>,
    #[prop(into)] busy: Signal<bool>,
    on_retract: UnsyncCallback<String>,
) -> impl IntoView {
    let sharing = use_sharing();
    let open_article = crate::navigation::use_article_opener();
    let confirm = RwSignal::new(false);
    let id = StoredValue::new(share.get_untracked().id);
    let title = NodeRef::<html::A>::new();
    let preferences = crate::preferences::use_preferences();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if share.get().retracted && confirm.get() {
            confirm.set(false);
            if let Some(link) = title.get() {
                let options = web_sys::FocusOptions::new();
                options.set_prevent_scroll(true);
                let _ = link.focus_with_options(&options);
            }
        }
    });
    view! { <li><p><a node_ref=title href=move ||format!("/item/{}",share.get().item_id) on:click=move |event|{event.prevent_default();open_article.run(share.get_untracked().item_id);preferences.settings_open.set(false);} >{move ||share.get().title}</a></p><p>{move ||format!("To {} · {}",share.get().to,share.get().date)}</p>{move ||share.get().note.map(|note| view! { <blockquote>{note}</blockquote> })}
        <button type="button" on:click=move |event|{crate::components::focus_dialog_trigger(&event);sharing.conversation.set(Some(id.get_value()));}>"Conversation"</button>
        <Show when=move || !share.get().retracted fallback=|| view! { <p>"Withdrawn"</p> }>
            <Show when=move || confirm.get() fallback=move || view! { <button type="button" disabled=move || busy.get() on:click=move |_| confirm.set(true)>"Withdraw share"</button> }><p>"Withdraw this vouch?"</p><button type="button" on:click=move |_| confirm.set(false)>"Cancel"</button><button type="button" disabled=move || busy.get() on:click=move |_| on_retract.run(id.get_value())>"Withdraw share"</button></Show>
        </Show>
    </li> }
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn request_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "ui-{}-{}",
        web_sys::js_sys::Date::now() as u64,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn request_token() -> String {
    "ssr-preview".into()
}
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl std::future::Future<Output = ()> + 'static) {
    leptos::task::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(_task: impl std::future::Future<Output = ()> + 'static) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_needs_a_followed_person_but_can_queue_while_they_are_offline() {
        let mut snapshot = api::PeopleSnapshot::default();
        assert!(!has_recipients(&snapshot));
        snapshot.people.push(api::PersonSummary {
            image: None,
            key: "test-key".into(),
            name: "Friend".into(),
            connected: false,
            followed: false,
            error: None,
        });
        assert!(!has_recipients(&snapshot));
        snapshot.people[0].followed = true;
        assert!(has_recipients(&snapshot));
        snapshot.people[0].connected = true;
        assert!(has_recipients(&snapshot));
        snapshot.people[0].followed = false;
        assert!(!has_recipients(&snapshot));
    }
}
