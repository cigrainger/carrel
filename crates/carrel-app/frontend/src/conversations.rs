//! Presentation and draft lifetime for private identities and share conversations.
use crate::{
    api,
    components::Modal,
    requests::Requests,
    sharing::{PendingLeave, request_token, use_sharing},
};
use leptos::{html, prelude::*};
use serde_json::json;

#[component]
pub(crate) fn Avatar(
    #[prop(into)] name: Signal<String>,
    #[prop(into)] image: Signal<Option<String>>,
) -> impl IntoView {
    view! {<span class="person-avatar" aria-hidden="true"><Show when=move ||image.get().is_some() fallback={move ||name.get().split_whitespace().take(2).filter_map(|part|part.chars().next()).collect::<String>().to_uppercase()}><img src=move ||image.get().unwrap_or_default() alt=""/></Show></span>}
}
#[component]
pub(crate) fn ProfileEditor() -> impl IntoView {
    let sharing = use_sharing();
    let profile = RwSignal::new(sharing.people.get_untracked().profile.unwrap_or_default());
    let dirty = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    Effect::new(move |_| {
        let fresh = sharing.people.get().profile;
        if !dirty.get_untracked()
            && let Some(fresh) = fresh
        {
            profile.set(fresh);
        }
    });
    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        let draft = profile.get_untracked();
        spawn(async move {
            let result = api::sharing_call::<()>(
                "save_profile",
                json!({"name":draft.name,"avatar":draft.avatar}),
            )
            .await;
            drop(write);
            sharing.refresh();
            if guard.accepts(request) {
                match result {
                    Ok(()) => {
                        dirty.try_set(false);
                        message.try_set(Some("Profile saved for the people you follow.".into()));
                    }
                    Err(e) => {
                        message.try_set(Some(e.message().into()));
                    }
                }
                busy.try_set(false);
            }
        });
    };
    view! {<section class="profile-editor"><h4>"Your name and picture"</h4><p>"Shared privately with the people you follow. Your public key remains your identity."</p>
        <Avatar name=Signal::derive(move ||profile.get().name) image=Signal::derive(move ||profile.get().image)/>
        <label for="profile-name">"Your display name (optional)"</label><input id="profile-name" maxlength="256" readonly=move ||busy.get() prop:value=move ||profile.get().name on:input=move |event|{dirty.set(true);profile.update(|p|p.name=event_target_value(&event));}/>
        <button type="button" disabled=move ||busy.get() on:click=move |_|{
            busy.set(true);message.set(None);let guard=requests.get_value();let request=guard.begin();
            spawn(async move {let result=api::sharing_call::<Option<api::ProfileSummary>>("choose_profile_picture",json!({})).await;
                if guard.accepts(request){match result{Ok(Some(picture))=>{dirty.try_set(true);profile.try_update(|p|{p.avatar=picture.avatar;p.image=picture.image;});},Ok(None)=>{},Err(e)=>{message.try_set(Some(e.message().into()));}}busy.try_set(false);}
            });
        }>"Choose picture…"</button>
        <Show when=move ||profile.get().avatar.is_some()><button type="button" disabled=move ||busy.get() on:click=move |_|{dirty.set(true);profile.update(|p|{p.avatar=None;p.image=None;});}>"Remove picture"</button></Show>
        <button type="button" disabled=move ||busy.get() || !dirty.get() on:click=save>"Save profile"</button>
        {move ||message.get().map(|message|view!{<p role="status">{message}</p>})}
    </section>}
}

#[component]
pub(crate) fn GroupsPanel() -> impl IntoView {
    let sharing = use_sharing();
    let id = RwSignal::new(request_token());
    let name = RwSignal::new(String::new());
    let members = RwSignal::new(Vec::<String>::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let editing = RwSignal::new(false);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let edit = UnsyncCallback::new(move |group: api::GroupSummary| {
        id.set(group.id);
        name.set(group.name);
        members.set(group.members);
        editing.set(true);
    });
    let remove = UnsyncCallback::new(move |group_id: String| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        spawn(async move {
            let result = api::sharing_call::<()>("remove_group", json!({"id":group_id})).await;
            drop(write);
            sharing.refresh();
            if guard.accepts(request) {
                if let Err(e) = result {
                    error.try_set(Some(e.message().into()));
                }
                busy.try_set(false);
            }
        });
    });
    view! {<section class="groups-panel"><h4>"Groups"</h4><p>"Choose people once for future shares. Each conversation keeps the people it was sent to."</p>
        <ul class="group-list"><For each=move ||sharing.people.get().groups key=|group|group.id.clone() let:group><GroupRow id=group.id on_edit=edit on_remove=remove busy=busy/></For></ul>
        <form on:submit=move |event|{event.prevent_default();if busy.get_untracked(){return;}busy.set(true);error.set(None);let guard=requests.get_value();let request=guard.begin();let write=writes.get_value().begin();
            let args=json!({"id":id.get_untracked(),"name":name.get_untracked(),"members":members.get_untracked()});
            spawn(async move{let result=api::sharing_call::<()>("save_group",args).await;drop(write);sharing.refresh();if guard.accepts(request){match result{Ok(())=>{id.try_set(request_token());name.try_set(String::new());members.try_set(Vec::new());editing.try_set(false);},Err(e)=>{error.try_set(Some(e.message().into()));}}busy.try_set(false);}});
        }><label for="group-name">{move ||if editing.get(){"Edit group name"}else{"New group name"}}</label><input id="group-name" maxlength="256" required=true readonly=move ||busy.get() prop:value=move ||name.get() on:input=move |event|name.set(event_target_value(&event))/>
            <fieldset disabled=move ||busy.get()><legend>"People in this group"</legend><For each={move ||sharing.people.get().people.into_iter().filter(|p|p.followed).collect::<Vec<_>>()} key=|person|person.key.clone() let:person>{let key=StoredValue::new(person.key);view!{<label class="group-member"><input type="checkbox" prop:checked=move ||members.get().contains(&key.get_value()) on:change=move |event|{let checked=event_target_checked(&event);members.update(|list|{list.retain(|k|k!=&key.get_value());if checked{list.push(key.get_value());}});}/>{person.name}</label>}}</For></fieldset>
            {move ||error.get().map(|error|view!{<p class="feed-error" role="alert">{error}</p>})}
            <button type="submit" disabled=move ||busy.get() ||members.get().is_empty() ||name.get().trim().is_empty()>"Save group"</button>
            <Show when=move ||editing.get()><button type="button" disabled=move ||busy.get() on:click=move |_|{id.set(request_token());name.set(String::new());members.set(Vec::new());editing.set(false);}>"Cancel editing"</button></Show>
        </form>
    </section>}
}
#[component]
fn GroupRow(
    id: String,
    on_edit: UnsyncCallback<api::GroupSummary>,
    on_remove: UnsyncCallback<String>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let sharing = use_sharing();
    let id = StoredValue::new(id);
    let confirm = RwSignal::new(false);
    let group = Memo::new(move |_| {
        sharing
            .people
            .get()
            .groups
            .into_iter()
            .find(|g| g.id == id.get_value())
    });
    view! {<li><strong>{move ||group.get().map(|g|g.name).unwrap_or_default()}</strong><p>{move ||group.get().map(|g|g.members.into_iter().map(|key|sharing.people.get().people.into_iter().find(|p|p.key==key).map(|p|p.name).unwrap_or(key)).collect::<Vec<_>>().join(", ")).unwrap_or_default()}</p><button type="button" disabled=move ||busy.get() on:click=move |_|{if let Some(group)=group.get_untracked(){on_edit.run(group);}}>"Edit group"</button>
        <Show when=move ||confirm.get() fallback=move ||view!{<button type="button" disabled=move ||busy.get() on:click=move |_|confirm.set(true)>"Remove group"</button>}><p>"Remove this group? Sent conversations keep their original participants."</p><button type="button" on:click=move |_|confirm.set(false)>"Cancel"</button><button type="button" disabled=move ||busy.get() on:click=move |_|on_remove.run(id.get_value())>"Remove group"</button></Show>
    </li>}
}

#[component]
pub(crate) fn ConversationDialog() -> impl IntoView {
    let sharing = use_sharing();
    let conversations = move || sharing.conversation.get().into_iter().collect::<Vec<_>>();
    view! {<For each=conversations key=|id|id.clone() let:id><ConversationForm id=id/></For>}
}

fn conversation_names(data: RwSignal<Option<api::Conversation>>) -> String {
    data.with(|data| {
        data.as_ref()
            .map(|thread| {
                thread
                    .participants
                    .iter()
                    .map(|person| person.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    })
}
fn conversation_reply(
    data: RwSignal<Option<api::Conversation>>,
    id: &str,
) -> Option<api::ConversationReply> {
    data.with(|data| {
        data.as_ref()
            .and_then(|thread| thread.replies.iter().find(|reply| reply.id == id).cloned())
    })
}
fn parent_context(data: RwSignal<Option<api::Conversation>>, id: &str) -> Option<String> {
    data.with(|data| {
        let thread = data.as_ref()?;
        let parent = thread
            .replies
            .iter()
            .find(|reply| reply.id == id)?
            .parent
            .as_ref()?;
        let reply = thread.replies.iter().find(|reply| reply.id == *parent)?;
        Some(format!(
            "{}: {}",
            reply.author.name,
            reply.body.as_deref().unwrap_or("Message removed")
        ))
    })
}

#[component]
fn ConversationOpening(
    data: RwSignal<Option<api::Conversation>>,
    on_read: UnsyncCallback<()>,
) -> impl IntoView {
    let sharing = use_sharing();
    let introduction = Memo::new(move |_| {
        let local = sharing.people.with(|people| people.key.clone());
        data.with(|data| {
            let Some(thread) = data else {
                return String::new();
            };
            let others = thread
                .participants
                .iter()
                .filter(|person| {
                    person.key != thread.opener.key && Some(&person.key) != local.as_ref()
                })
                .map(|person| person.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if local.as_ref() == Some(&thread.opener.key) {
                format!("You shared this article with {others}.")
            } else if thread
                .participants
                .iter()
                .any(|person| Some(&person.key) == local.as_ref())
            {
                if others.is_empty() {
                    format!("{} shared this article with you.", thread.opener.name)
                } else {
                    format!(
                        "{} shared this article with you and {others}.",
                        thread.opener.name
                    )
                }
            } else {
                format!("{} shared this article with {others}.", thread.opener.name)
            }
        })
    });
    view! {
        <div class="conversation-context" data-share-id=move ||data.with(|data|data.as_ref().map(|thread|thread.id.clone()))>
            <h4>{move ||data.with(|data|data.as_ref().map(|thread|thread.title.clone()).unwrap_or_default())}</h4>
            <p>{move ||introduction.get()}</p>
            <p>{move ||data.with(|data|data.as_ref().map(|thread|thread.date.clone()).unwrap_or_default())}</p>
            {move ||data.with(|data|data.as_ref().and_then(|thread|thread.note.clone())).map(|note|view!{<blockquote dir="auto">{note}</blockquote>})}
            <details>
                <summary>"Original audience"</summary>
                <p class="capture-notice">"This audience is fixed for the share. Each person chooses whether to take part. Replies may wait while desktops are offline."</p>
                <ul class="conversation-people"><For each=move ||data.with(|data|data.as_ref().map(|thread|thread.participants.clone()).unwrap_or_default()) key=|person|person.key.clone() let:person>
                    {let key=StoredValue::new(person.key.clone());let fallback=StoredValue::new(person);view!{
                        <li><crate::person_card::ConversationPersonButton share_id=data.with_untracked(|data|data.as_ref().map(|thread|thread.id.clone()).unwrap_or_default()) person=Signal::derive(move ||data.with(|data|data.as_ref().and_then(|thread|thread.participants.iter().find(|person|person.key==key.get_value()).cloned()).unwrap_or_else(||fallback.get_value())))/></li>
                    }}
                </For></ul>
            </details>
            <a href=move ||data.with(|data|data.as_ref().map(|thread|format!("/item/{}",thread.item_id)).unwrap_or_default()) on:click=move |event|{event.prevent_default();on_read.run(());}>"Read article"</a>
        </div>
    }
}

#[component]
pub(crate) fn ArticleConversations(item_id: String) -> impl IntoView {
    let sharing = use_context::<crate::sharing::Sharing>();
    let item = StoredValue::new(item_id);
    let links = RwSignal::new(Vec::<api::ConversationLink>::new());
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let load = UnsyncCallback::new(move |()| {
        let guard = requests.get_value();
        let request = guard.begin();
        let id = item.get_value();
        spawn(async move {
            match api::sharing_call::<Vec<api::ConversationLink>>(
                "list_article_conversations",
                json!({"itemId":id}),
            )
            .await
            {
                Ok(rows) if guard.accepts(request) => {
                    links.try_set(rows);
                    error.try_set(None);
                }
                Err(e) if guard.accepts(request) => {
                    error.try_set(Some(e.message().into()));
                }
                _ => {}
            }
        });
    });
    Effect::new(move |_| {
        if let Some(sharing) = sharing {
            sharing.history_revision().get();
        }
        load.run(());
    });
    view! {
        <Show when=move ||!links.get().is_empty()>
            <section class="article-conversations">
                <h3>"Conversations"</h3>
                <ul><For each=move ||links.get() key=|thread|thread.id.clone() let:thread>
                    {let id=StoredValue::new(thread.id.clone());let fallback=StoredValue::new(thread);let thread=Signal::derive(move ||links.with(|links|links.iter().find(|thread|thread.id==id.get_value()).cloned()).unwrap_or_else(||fallback.get_value()));view! {
                    <li><button type="button" disabled=sharing.is_none() on:click=move |event|{
                        crate::components::focus_dialog_trigger(&event);
                        if let Some(sharing)=sharing {sharing.conversation.set(Some(id.get_value()));}
                    }>{move ||thread.with(|thread|format!("{} · {}",thread.from,thread.date))}</button>{move ||thread.with(|thread|thread.access.reason.clone().or_else(||thread.closed.then(||"This share was withdrawn. Its saved history remains available.".into()))).map(|reason|view!{<p class="capture-notice">{reason}</p>})}</li>
                    }}
                </For></ul>
            </section>
        </Show>
        {move ||error.get().map(|message|view! {
            <p class="feed-error">{message}</p>
            <button type="button" on:click=move |_|load.run(())>"Retry conversations"</button>
        })}
    }
}
#[component]
fn ConversationForm(id: String) -> impl IntoView {
    let sharing = use_sharing();
    let open_article = crate::navigation::use_article_opener();
    let preferences = crate::preferences::use_preferences();
    sharing.drafts.get_value().load(&id);
    let id = StoredValue::new(id);
    let data = RwSignal::new(None::<api::Conversation>);
    provide_context(data);
    let error = RwSignal::new(None::<String>);
    let local_busy = RwSignal::new(false);
    let confirm_leave = RwSignal::new(false);
    let draft_state = Memo::new(move |_| sharing.drafts.get_value().view(&id.get_value()));
    let busy = Signal::derive(move || local_busy.get() || draft_state.with(|state| state.busy));
    let ready = Memo::new(move |_| draft_state.with(|state| state.draft.is_some()));
    let body = Memo::new(move |_| {
        draft_state.with(|state| {
            state
                .draft
                .as_ref()
                .map(|draft| draft.body.clone())
                .unwrap_or_default()
        })
    });
    let parent = Memo::new(move |_| {
        draft_state.with(|state| state.draft.as_ref().and_then(|draft| draft.parent.clone()))
    });
    let notice = RwSignal::new(None::<String>);
    let input = NodeRef::<html::Textarea>::new();
    let submit = NodeRef::<html::Button>::new();
    let done = NodeRef::<html::Button>::new();
    let pending_leave = Memo::new(move |_| {
        sharing
            .pending_conversation_leaves
            .with(|pending| pending.get(&id.get_value()).copied())
    });
    let can_reply = Memo::new(move |_| {
        data.with(|data| data.as_ref().is_some_and(|thread| thread.access.can_reply))
            && pending_leave.get().is_none()
    });
    let can_leave = Memo::new(move |_| {
        data.with(|data| data.as_ref().is_some_and(|thread| thread.access.can_leave))
            && pending_leave.get().is_none()
    });
    let unavailable_reason = Memo::new(move |_| -> String {
        if pending_leave.get() == Some(PendingLeave::Saving) {
            "Saving your choice. Your unsent draft stays here.".into()
        } else if pending_leave.get() == Some(PendingLeave::Confirming) {
            "Refresh this conversation to confirm its current availability. Your unsent draft stays here.".into()
        } else {
            data.with(|data| {
                data.as_ref().and_then(|thread| {
                    thread.access.reason.clone().or_else(|| {
                        thread.closed.then(|| {
                            "This share was withdrawn. Its saved history remains available.".into()
                        })
                    })
                })
            })
            .unwrap_or_else(|| {
                "Discussion is not available yet. You can still read the article and opening note."
                    .into()
            })
        }
    });
    #[cfg(target_arch = "wasm32")]
    {
        let previous = StoredValue::new((false, false, false, false, false));
        Effect::new(move |_| {
            let current = (
                can_reply.get(),
                can_leave.get(),
                ready.get(),
                draft_state.with(|state| state.busy),
                draft_state.with(|state| state.error.is_some()),
            );
            let before = previous.get_value();
            previous.set_value(current);
            if before != current
                && let Some(button) = done.get_untracked()
            {
                set_timeout(
                    move || {
                        use wasm_bindgen::JsCast;
                        let Some(document) = web_sys::window().and_then(|window| window.document())
                        else {
                            return;
                        };
                        // A nested person card or palette keeps its own focus.
                        if !button.is_connected()
                            || document
                                .active_element()
                                .is_some_and(|element| element.tag_name() != "BODY")
                        {
                            return;
                        }
                        let field = button
                            .closest("dialog")
                            .ok()
                            .flatten()
                            .and_then(|dialog| {
                                dialog.query_selector("#closed-reply-draft, #cancel-leave-conversation, #conversation-reply").ok().flatten()
                            })
                            .and_then(|field| field.dyn_into::<web_sys::HtmlElement>().ok());
                        let options = web_sys::FocusOptions::new();
                        options.set_prevent_scroll(true);
                        if let Some(field) = field.filter(|field| field.is_connected()) {
                            let _ = field.focus_with_options(&options);
                        } else if button.is_connected() {
                            let _ = button.focus_with_options(&options);
                        }
                    },
                    std::time::Duration::ZERO,
                );
            }
        });
    }
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let mutations = StoredValue::new(Requests::default());
    let cleanup = mutations.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let load = UnsyncCallback::new(move |()| {
        if local_busy.get_untracked() {
            return;
        }
        let guard = requests.get_value();
        let request = guard.begin();
        let target = id.get_value();
        let revision = sharing.history_revision().get_untracked();
        spawn(async move {
            let result =
                api::sharing_call::<api::Conversation>("get_conversation", json!({"id":target}))
                    .await;
            if guard.accepts(request) && revision == sharing.history_revision().get_untracked() {
                match result {
                    Ok(fresh) => {
                        data.try_set(Some(fresh));
                        sharing.pending_conversation_leaves.try_update(|pending| {
                            if pending.get(&target) == Some(&PendingLeave::Confirming) {
                                pending.remove(&target);
                            }
                        });
                    }
                    Err(e) => {
                        error.try_set(Some(e.message().into()));
                    }
                }
            }
        });
    });
    Effect::new(move |_| {
        sharing.history_revision().get();
        load.run(());
    });
    #[cfg(target_arch = "wasm32")]
    if let Ok(interval) =
        set_interval_with_handle(move || load.run(()), std::time::Duration::from_secs(3))
    {
        on_cleanup(move || interval.clear());
    }
    let reply = UnsyncCallback::new(move |reply_id: String| {
        if !can_reply.get_untracked() || busy.get_untracked() || !ready.get_untracked() {
            return;
        }
        sharing
            .drafts
            .get_value()
            .edit(&id.get_value(), |draft| draft.parent = Some(reply_id));
        if let Some(input) = input.get_untracked() {
            let _ = input.focus();
        }
    });
    let removed = UnsyncCallback::new(move |reply_id: String| {
        if busy.get_untracked() {
            return;
        }
        local_busy.set(true);
        requests.get_value().invalidate();
        error.set(None);
        let guard = mutations.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        let target = id.get_value();
        spawn(async move {
            let result = api::sharing_call::<()>(
                "remove_reply",
                json!({"shareId":target,"replyId":reply_id}),
            )
            .await;
            drop(write);
            if guard.accepts(request) {
                if let Err(e) = result {
                    error.try_set(Some(e.message().into()));
                } else {
                    #[cfg(target_arch = "wasm32")]
                    if can_reply.get_untracked()
                        && let Some(input) = input.get_untracked()
                    {
                        let options = web_sys::FocusOptions::new();
                        options.set_prevent_scroll(true);
                        let _ = input.focus_with_options(&options);
                    }
                }
                local_busy.try_set(false);
                load.run(());
            }
        });
    });
    let leave = UnsyncCallback::new(move |()| {
        if busy.get_untracked() || !can_leave.get_untracked() {
            return;
        }
        local_busy.set(true);
        sharing.pending_conversation_leaves.update(|pending| {
            pending.insert(id.get_value(), PendingLeave::Saving);
        });
        sharing.history_changed();
        error.set(None);
        notice.set(None);
        requests.get_value().invalidate();
        let guard = mutations.get_value();
        let request = guard.begin();
        let write = writes.get_value().begin();
        let target = id.get_value();
        spawn(async move {
            let result =
                api::sharing_call::<()>("leave_conversation", json!({"shareId":target})).await;
            drop(write);
            sharing.pending_conversation_leaves.try_update(|pending| {
                pending.insert(target, PendingLeave::Confirming);
            });
            sharing.history_changed();
            if !guard.accepts(request) {
                return;
            }
            match result {
                Ok(()) => {
                    confirm_leave.try_set(false);
                }
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                }
            }
            // No earlier read can reopen the composer while confirming this choice.
            requests.get_value().invalidate();
            local_busy.try_set(false);
            load.run(());
        });
    });
    let leave_button = NodeRef::<html::Button>::new();
    let keep_button = NodeRef::<html::Button>::new();
    let discard_button = NodeRef::<html::Button>::new();
    let cancel_discard_button = NodeRef::<html::Button>::new();
    let confirm_discard = RwSignal::new(None::<crate::sharing::ConversationDraft>);
    let close = UnsyncCallback::new(move |()| sharing.conversation.set(None));
    view! {<Modal label="Private conversation" class="feed-dialog conversation-dialog" on_cancel=close>
        <header><h3>"Conversation"</h3><button node_ref=done type="button" on:click=move |_|close.run(())>"Done"</button></header>
        {move ||error.get().map(|e|view!{<p class="feed-error" role="alert">{e}</p><button type="button" on:click=move |_|{error.set(None);load.run(());}>"Refresh conversation"</button>})}
        <Show when=move ||draft_state.with(|state|state.loading)><p role="status">"Loading your saved reply…"</p></Show>
        {move ||draft_state.with(|state|state.error.clone()).map(|message|view! {
            <p class="feed-error" role="alert">{message}</p>
            <Show when=move ||!ready.get()><button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().load(&id.get_value())>"Retry loading draft"</button></Show>
            <Show when=move ||ready.get() && draft_state.with(|state|!state.conflict && !state.failed_send)><button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().retry_save(&id.get_value())>"Retry saving draft"</button></Show>
            <Show when=move ||draft_state.with(|state|state.failed_send)>
                <p>"Retry send checks the exact reply you submitted. It may already have been saved. Newer text stays here."</p>
                <button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().send(&id.get_value())>"Retry send"</button>
                <crate::components::copy_text::CopyTextField id="failed-send-draft" label="Reply awaiting confirmation" button="Copy submitted reply" text=Signal::derive(move ||draft_state.with(|state|state.submitted.as_ref().map(|draft|draft.body.clone()).unwrap_or_default()))/>
                <Show when=move ||draft_state.with(|state|state.draft!=state.submitted)><crate::components::copy_text::CopyTextField id="buffered-reply-draft" label="Your newer composition" button="Copy unsent reply" text=Signal::derive(move ||body.get())/></Show>
            </Show>
            <Show when=move ||draft_state.with(|state|state.conflict && !state.reviewed)><button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().review(&id.get_value())>"Review saved draft"</button></Show>
        })}
        <Show when=move ||draft_state.with(|state|state.conflict && state.reviewed)>
            <Show when=move ||draft_state.with(|state|state.saved.is_some()) fallback=||view!{<p>"The saved draft was cleared."</p>}>
                <crate::components::copy_text::CopyTextField id="saved-reply-draft" label="Draft saved on this desktop" button="Copy saved draft" text=Signal::derive(move ||draft_state.with(|state|state.saved.as_ref().map(|draft|draft.body.clone()).unwrap_or_default()))/>
                <Show when=move ||draft_state.with(|state|state.saved.as_ref().is_some_and(|draft|draft.parent.is_some()))>
                    <p>"Saved reply context"</p><blockquote>{move ||draft_state.with(|state|state.saved.as_ref().and_then(|draft|draft.parent.clone())).and_then(|parent|conversation_reply(data,&parent)).and_then(|reply|reply.body).unwrap_or_else(||"Message removed or unavailable".into())}</blockquote>
                </Show>
            </Show>
            <Show when=move ||!draft_state.with(|state|state.failed_send)><button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().retry_save(&id.get_value())>"Keep my draft"</button></Show>
        </Show>
        <Show when=move ||data.with(|data|data.is_some())><ConversationOpening data=data on_read=UnsyncCallback::new(move |()|{if let Some(conversation)=data.get_untracked(){open_article.run(conversation.item_id);}sharing.conversation.set(None);preferences.settings_open.set(false);})/>
            <Show when=move ||can_leave.get()>
                <Show when=move ||confirm_leave.get() fallback=move ||view! {
                    <button node_ref=leave_button type="button" disabled=move ||busy.get() on:click=move |event|{
                        crate::components::focus_dialog_trigger(&event);confirm_leave.set(true);
                        #[cfg(target_arch = "wasm32")]
                        set_timeout(move ||{if let Some(button)=keep_button.get_untracked(){let options=web_sys::FocusOptions::new();options.set_prevent_scroll(true);let _=button.focus_with_options(&options);}},std::time::Duration::ZERO);
                    }>"Leave conversation"</button>
                }>
                    <p>"Leave this conversation? New discussion will stop for you. The saved article and accepted history stay here, and you can copy your unsent reply. Leaving cannot erase copies already received."</p>
                    <button node_ref=keep_button id="cancel-leave-conversation" type="button" disabled=move ||busy.get() on:click=move |_|{
                        confirm_leave.set(false);
                        #[cfg(target_arch = "wasm32")]
                        set_timeout(move ||{if let Some(button)=leave_button.get_untracked(){let options=web_sys::FocusOptions::new();options.set_prevent_scroll(true);let _=button.focus_with_options(&options);}},std::time::Duration::ZERO);
                    }>"Cancel"</button>
                    <button type="button" disabled=move ||busy.get() on:click=move |_|leave.run(())>"Leave conversation"</button>
                </Show>
            </Show>
            <ol class="conversation-replies"><For each=move ||data.with(|data|data.as_ref().map(|thread|thread.replies.clone()).unwrap_or_default()) key=|reply|reply.id.clone() let:message>{let reply_id=StoredValue::new(message.id.clone());let fallback=StoredValue::new(message);view!{<ReplyRow share_id=id.get_value() message=Signal::derive(move ||conversation_reply(data,&reply_id.get_value()).unwrap_or_else(||fallback.get_value())) context=Signal::derive(move ||parent_context(data,&reply_id.get_value())) can_reply=Signal::derive(move ||can_reply.get() && ready.get()) on_reply=reply on_remove=removed busy=busy/>}}</For></ol>
            <Show when=move ||parent.get().is_some()><div class="reply-context"><p>"Replying to "{move ||parent.get().and_then(|id|conversation_reply(data,&id)).map(|r|r.author.name).unwrap_or_default()}</p><blockquote>{move ||parent.get().and_then(|id|conversation_reply(data,&id)).and_then(|r|r.body).unwrap_or_else(||"Message removed or unavailable".into())}</blockquote><Show when=move ||can_reply.get()><button type="button" disabled=move ||busy.get() on:click=move |_|sharing.drafts.get_value().edit(&id.get_value(),|draft|draft.parent=None)>"Reply to conversation instead"</button></Show></div></Show>
            <Show when=move ||can_reply.get() && ready.get() fallback=move ||view!{<Show when=move ||!can_reply.get()><p>{move ||unavailable_reason.get()}</p></Show>}>
                <form on:keydown=move |event|{if (event.meta_key() ||event.ctrl_key()) &&event.key()=="Enter"{event.prevent_default();event.stop_propagation();if let Some(button)=submit.get_untracked(){button.click();}}} on:submit=move |event|{
                    event.prevent_default();if !can_reply.get_untracked()||!ready.get_untracked()||busy.get_untracked()||body.get_untracked().trim().is_empty()||draft_state.with_untracked(|state|state.conflict||state.failed_send){return;}
                    error.set(None);notice.set(None);requests.get_value().invalidate();sharing.drafts.get_value().send(&id.get_value());
                }>
                    <p class="capture-notice">"Your reply is addressed to "{move ||conversation_names(data)}". Each person chooses whether to take part. Delivery may wait while desktops are offline."</p>
                    <label for="conversation-reply">"Your reply"</label><textarea node_ref=input id="conversation-reply" dir="auto" rows="4" maxlength="65536" readonly=move ||local_busy.get()||draft_state.with(|state|state.discarding) prop:value=move ||body.get() on:input=move |event|sharing.drafts.get_value().edit(&id.get_value(),|draft|draft.body=event_target_value(&event))/><button node_ref=submit type="submit" disabled=move ||!can_reply.get()||busy.get()||body.get().trim().is_empty()||draft_state.with(|state|state.conflict||state.failed_send)>"Send reply"</button>
                </form>
            </Show>
            {move ||notice.get().or_else(||draft_state.with(|state|state.notice.clone())).map(|text|view!{<p role="status">{text}</p>})}
        </Show>
        <Show when=move ||ready.get() && !can_reply.get() && !body.get().is_empty()><crate::components::copy_text::CopyTextField id="closed-reply-draft" label="Your unsent draft" button="Copy unsent reply" text=Signal::derive(move ||body.get())/></Show>
        <Show when=move ||ready.get() && (!body.get().is_empty() || parent.get().is_some() || draft_state.with(|state|state.unsaved||state.failed_send||state.saved.is_some()))>
            <p class="capture-notice">"Drafts stay on this desktop and are included in its encrypted backup."</p>
            <Show when=move ||confirm_discard.get().is_some() fallback=move ||view! {
                <button node_ref=discard_button type="button" disabled=move ||busy.get()||draft_state.with(|state|state.conflict&&!state.reviewed) on:click=move |event|{
                    crate::components::focus_dialog_trigger(&event);
                    confirm_discard.set(draft_state.with_untracked(|state|state.draft.clone()));
                    #[cfg(target_arch = "wasm32")]
                    set_timeout(move ||{if let Some(button)=cancel_discard_button.get_untracked(){let options=web_sys::FocusOptions::new();options.set_prevent_scroll(true);let _=button.focus_with_options(&options);}},std::time::Duration::ZERO);
                }>"Discard draft"</button>
            }>
                <p>"Discard this conversation’s draft on this desktop? This removes its unsent text and reply context. It cannot undo a reply already sent."</p>
                <button node_ref=cancel_discard_button type="button" disabled=move ||busy.get() on:click=move |_|{
                    confirm_discard.set(None);
                    #[cfg(target_arch = "wasm32")]
                    set_timeout(move ||{if let Some(button)=discard_button.get_untracked(){let options=web_sys::FocusOptions::new();options.set_prevent_scroll(true);let _=button.focus_with_options(&options);}},std::time::Duration::ZERO);
                }>"Cancel"</button>
                <button type="button" disabled=move ||busy.get() on:click=move |_|{
                    if let Some(reviewed)=confirm_discard.get_untracked(){sharing.drafts.get_value().discard(&id.get_value(),&reviewed);}
                    confirm_discard.set(None);
                }>"Discard draft"</button>
            </Show>
        </Show>
    </Modal>}
}
#[component]
fn ReplyRow(
    share_id: String,
    message: Signal<api::ConversationReply>,
    context: Signal<Option<String>>,
    can_reply: Signal<bool>,
    on_reply: UnsyncCallback<String>,
    on_remove: UnsyncCallback<String>,
    busy: Signal<bool>,
) -> impl IntoView {
    let conversation = expect_context::<RwSignal<Option<api::Conversation>>>();
    let share_id = StoredValue::new(share_id);
    let id = StoredValue::new(message.get_untracked().id);
    let confirm = RwSignal::new(false);
    let proof = RwSignal::new(None::<Vec<api::ReplyProvenance>>);
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    view! {<li><div class="conversation-author"><crate::person_card::ConversationPersonButton share_id=share_id.get_value() person=Signal::derive(move || {
            let author=message.with(|message|message.author.clone());
            conversation.with(|data|data.as_ref().and_then(|thread|thread.participants.iter().find(|person|person.key==author.key).cloned())).unwrap_or(author)
        })/><time>{move ||message.get().date}</time></div>
        {move ||context.get().map(|text|view!{<blockquote class="reply-parent-context">{text}</blockquote>})}
        <p class="reply-body" dir="auto">{move ||message.get().body.unwrap_or_else(||"Message removed by its author.".into())}</p>
        <Show when=move ||can_reply.get()><button type="button" disabled=move ||busy.get() on:click=move |_|on_reply.run(id.get_value())>"Reply"</button></Show>
        <Show when=move ||message.get().can_remove><Show when=move ||confirm.get() fallback=move ||view!{<button type="button" disabled=move ||busy.get() on:click=move |_|confirm.set(true)>"Remove message"</button>}><p>"Remove your message? People may keep copies already received."</p><button type="button" on:click=move |_|confirm.set(false)>"Cancel"</button><button type="button" disabled=move ||busy.get() on:click=move |_|on_remove.run(id.get_value())>"Remove message"</button></Show></Show>
        <details class="reply-provenance" on:toggle=move |_|{if proof.get_untracked().is_some(){return;}let guard=requests.get_value();let request=guard.begin();spawn(async move{let result=api::sharing_call::<Vec<api::ReplyProvenance>>("reply_provenance",json!({"shareId":share_id.get_value(),"replyId":id.get_value()})).await;if guard.accepts(request){match result{Ok(records)=>{proof.try_set(Some(records));},Err(e)=>{error.try_set(Some(e.message().into()));}}}});}>
            <summary>"Provenance and reply ancestry"</summary><p>"Original signed messages, from this reply back to its earliest parent."</p>{move ||error.get().map(|e|view!{<p role="alert">{e}</p>})}<ol>{move ||proof.get().unwrap_or_default().into_iter().map(|p|view!{<li><p>{p.date}</p><dl><dt>"Message"</dt><dd><code>{p.id}</code></dd><dt>"Author key"</dt><dd><code>{p.author}</code></dd><dt>"Certified desktop key"</dt><dd><code>{p.device}</code></dd><dt>"Signed record"</dt><dd><code>{p.record}</code></dd><dt>"Signed parent messages"</dt><dd><code>{p.parents.join(", ")}</code></dd></dl></li>}).collect_view()}</ol>
        </details>
    </li>}
}

#[component]
pub(crate) fn NoteProvenance(note_id: String) -> impl IntoView {
    let id = StoredValue::new(note_id);
    let records = RwSignal::new(None::<Vec<api::ReplyProvenance>>);
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    view! {<details class="reply-provenance" on:toggle=move |_|{if records.get_untracked().is_some(){return;}let guard=requests.get_value();let request=guard.begin();spawn(async move{let result=api::sharing_call::<Vec<api::ReplyProvenance>>("note_provenance",json!({"noteId":id.get_value()})).await;if guard.accepts(request){match result{Ok(history)=>{records.try_set(Some(history));error.try_set(None);},Err(e)=>{error.try_set(Some(e.message().into()));}}}});}>
        <summary>"Revision provenance"</summary><p>"Every current branch and its signed ancestors. Legacy versions have no signed parent links."</p>
        {move ||error.get().map(|error|view!{<p role="alert">{error}</p>})}
        <ul>{move ||records.get().unwrap_or_default().into_iter().map(|record|view!{<li><p>{record.date}</p><dl><dt>"Revision"</dt><dd><code>{record.id}</code></dd><dt>"Original author key"</dt><dd><code>{record.author}</code></dd><dt>"Signing desktop key"</dt><dd><code>{record.device}</code></dd><dt>"Signed parent revisions"</dt><dd><code>{record.parents.join(", ")}</code></dd></dl></li>}).collect_view()}</ul>
    </details>}
}
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl std::future::Future<Output = ()> + 'static) {
    leptos::task::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(_task: impl std::future::Future<Output = ()> + 'static) {}
