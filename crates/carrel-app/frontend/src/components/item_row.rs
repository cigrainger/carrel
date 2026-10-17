//! Item-list rendering with native, keyboard-accessible controls.

use crate::api::ItemSummary;
use crate::components::{Icon, IconName};
use leptos::prelude::*;

#[component]
pub fn ItemList(
    #[prop(into)] items: Signal<Vec<ItemSummary>>,
    #[prop(into)] selected_index: Signal<usize>,
    aria_label: String,
    on_open: UnsyncCallback<usize>,
    on_read: UnsyncCallback<usize>,
    on_star: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
    on_conversation: UnsyncCallback<String>,
) -> impl IntoView {
    view! {
        <ul class="item-list" aria-label=aria_label>
            <For
                each=move || { items.get().into_iter().enumerate().collect::<Vec<_>>() }
                key=|(_, item)| item.id.clone()
                let:row
            >
                {
                    let (initial_index, item) = row;
                    let id = item.id.clone();
                    let index = Memo::new(move |_| items.with(|items| items.iter().position(|item| item.id == id).unwrap_or(initial_index)));
                    let selected = Signal::derive(move || selected_index.get() == index.get());
                    let repeated = Signal::derive(move || items.with(|items| {
                        let current = index.get();
                        current > 0 && items.get(current).and_then(|item| item.date.as_ref()).zip(items.get(current - 1).and_then(|item| item.date.as_ref())).is_some_and(|(date, previous)| date.iso == previous.iso)
                    }));
                    let item = row_data(items, item);
                    view! { <ItemRow index=index item=item selected=selected repeated=repeated on_open=on_open on_read=on_read on_star=on_star on_select=on_select on_conversation=on_conversation/> }
                }
            </For>
        </ul>
    }
}

fn row_data(items: Signal<Vec<ItemSummary>>, fallback: ItemSummary) -> Memo<ItemSummary> {
    Memo::new(move |_| {
        items.with(|items| {
            items
                .iter()
                .find(|item| item.id == fallback.id)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        })
    })
}

#[component]
pub fn ItemRow(
    #[prop(into)] index: Signal<usize>,
    #[prop(into)] item: Signal<ItemSummary>,
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] repeated: Signal<bool>,
    on_open: UnsyncCallback<usize>,
    on_read: UnsyncCallback<usize>,
    on_star: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
    on_conversation: UnsyncCallback<String>,
) -> impl IntoView {
    view! {
        <li class="item-row" class:is-selected=move || selected.get() data-item-id=move || item.get().id on:focusin=move |_| on_select.run(index.get_untracked())>
            {move || item.get().date.map(|date| view! {
                <time class="item-date" class:is-repeat=move || repeated.get() datetime=date.iso aria-label=date.label.clone() title=date.label>
                    <span class="calendar-day">{date.day}</span><span class="calendar-month">{date.caption.map(|caption| view! { <span class="calendar-origin">{caption}</span> })}{date.month}<br/>{date.year}</span>
                </time>
            }.into_any()).unwrap_or_else(|| view! { <span class="item-date date-unavailable">"Date unknown"</span> }.into_any())}
            <div class="item-row-body">
                <p class="item-meta">
                    <span class="read-indicator" role="img" aria-label=move || if item.get().read_state == "unread" { "Unread" } else { "Read" }>{move || if item.get().read_state == "unread" { "●" } else { "○" }}</span>
                    <span>{move || item.get().source_name}</span><span>{move || item.get().length_label}</span>
                </p>
                <h2><button class="item-open" type="button" on:click=move |_| on_open.run(index.get_untracked())>{move || item.get().title}</button></h2>
                <p class="item-excerpt">{move || item.get().summary.unwrap_or_default()}</p>
                {move || item.get().shared.map(|share| view! { <div class="received-vouch"><p>{format!("From {}", share.from)}</p>{share.note.map(|note| view! { <blockquote>{note}</blockquote> })}<button type="button" on:click=move |event|{super::focus_dialog_trigger(&event);on_conversation.run(share.id.clone());}>"Conversation"</button></div> })}
            </div>
                <div class="item-row-actions">
                    <button class="item-state-action" type="button"
                        aria-label=move || if item.get().read_state == "read" { "Mark unread" } else { "Mark read" }
                        aria-pressed=move || (item.get().read_state == "read").to_string()
                        title=move || if item.get().read_state == "read" { "Mark unread (⇧M)" } else { "Mark read (M)" }
                        on:click=move |_| on_read.run(index.get_untracked())>
                        <Icon name=IconName::Read/>
                    </button>
                    <button class="item-star-action" type="button"
                        aria-label=move || if item.get().starred { "Unstar" } else { "Star" }
                        title=move || if item.get().starred { "Unstar (S)" } else { "Star (S)" }
                        aria-pressed=move || item.get().starred.to_string() on:click=move |_| on_star.run(index.get_untracked())>
                        <Icon name=IconName::Star/>
                    </button>
                </div>
        </li>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_row_tracks_metadata_and_state_updates_without_changing_identity() {
        Owner::new().with(|| {
            let initial = ItemSummary {
                shared: None,
                id: "one".into(),
                title: "Before".into(),
                source_name: "Example".into(),
                length_label: "5 min".into(),
                time_label: "Now".into(),
                date: None,
                read_state: "unread".into(),
                starred: false,
                primary_url: None,
                summary: None,
                discovered_at_micros: 1,
            };
            let items = RwSignal::new(vec![initial.clone()]);
            let row = row_data(items.into(), initial.clone());
            assert_eq!(row.get_untracked().title, "Before");
            items.update(|items| {
                items[0].title = "After".into();
                items[0].summary = Some("Updated excerpt".into());
                items[0].starred = true;
                items[0].read_state = "read".into();
            });
            let updated = row.get_untracked();
            assert_eq!(updated.title, "After");
            assert_eq!(updated.summary.as_deref(), Some("Updated excerpt"));
            assert!(updated.starred);
            assert_eq!(updated.read_state, "read");
            assert_eq!(updated.id, "one");
            items.update(|items| {
                let mut inserted = initial.clone();
                inserted.id = "new".into();
                items.insert(0, inserted);
            });
            assert_eq!(row.get_untracked(), updated);
        });
    }

    #[test]
    fn item_list_exposes_open_read_and_star_as_native_buttons() {
        let html = Owner::new().with(|| view! {
            <ItemList
                items=Signal::stored(vec![ItemSummary {
                    shared: None,
                    id: "item-1".into(), title: "The shape of a small reader".into(), source_name: "Example".into(),
                    length_label: "5 min".into(), time_label: "2h ago".into(), date: None, read_state: "unread".into(), starred: true,
                    primary_url: None, summary: Some("A compact summary.".into()), discovered_at_micros: 1,
                }])
                selected_index=Signal::stored(0)
                aria_label=String::from("Today")
                on_open=UnsyncCallback::new(|_: usize| {})
                on_read=UnsyncCallback::new(|_: usize| {})
                on_star=UnsyncCallback::new(|_: usize| {})
                on_select=UnsyncCallback::new(|_: usize| {})
                on_conversation=UnsyncCallback::new(|_:String|{})
            />
        }.to_html());
        assert!(html.contains("The shape of a small reader</button>"));
        assert!(html.contains("aria-label=\"Mark read\""));
        assert!(html.contains("aria-label=\"Unstar\""));
        assert!(html.contains("<svg"));
        assert!(html.contains("aria-hidden=\"true\""));
        assert!(html.contains("aria-pressed=\"true\""));
        assert!(html.contains("A compact summary."));
    }
}
