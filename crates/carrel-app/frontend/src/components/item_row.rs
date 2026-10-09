//! Item-list rendering with native, keyboard-accessible controls.

use crate::api::ItemSummary;
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
) -> impl IntoView {
    view! {
        <ul class="item-list" aria-label=aria_label>
            <For
                each=move || { items.get().into_iter().enumerate().collect::<Vec<_>>() }
                key=|(index, item)| (*index, item.id.clone())
                let:row
            >
                {
                    let (index, item) = row;
                    let selected = Signal::derive(move || selected_index.get() == index);
                    let item = row_data(items, index, item);
                    view! { <ItemRow index=index item=item selected=selected on_open=on_open on_read=on_read on_star=on_star on_select=on_select/> }
                }
            </For>
        </ul>
    }
}

fn row_data(
    items: Signal<Vec<ItemSummary>>,
    index: usize,
    fallback: ItemSummary,
) -> Memo<ItemSummary> {
    Memo::new(move |_| {
        items.with(|items| {
            items
                .get(index)
                .filter(|item| item.id == fallback.id)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        })
    })
}

#[component]
pub fn ItemRow(
    index: usize,
    #[prop(into)] item: Signal<ItemSummary>,
    #[prop(into)] selected: Signal<bool>,
    on_open: UnsyncCallback<usize>,
    on_read: UnsyncCallback<usize>,
    on_star: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
) -> impl IntoView {
    view! {
        <li class="item-row" class:is-selected=move || selected.get() data-item-id=move || item.get().id on:focusin=move |_| on_select.run(index)>
            <span class="read-indicator" aria-hidden="true">{move || if item.get().read_state == "unread" { "●" } else { "○" }}</span>
            <div class="item-row-body">
                <h2><button class="item-open" type="button" on:click=move |_| on_open.run(index)>{move || item.get().title}</button></h2>
                <p class="item-meta">
                    <span>{move || item.get().source_name}</span><span>{move || item.get().length_label}</span><span>{move || item.get().time_label}</span>
                </p>
                <p class="item-excerpt">{move || item.get().summary.unwrap_or_default()}</p>
                <div class="item-row-actions">
                    <button class="item-state-action" type="button" on:click=move |_| on_read.run(index)>
                        {move || if item.get().read_state == "read" { "Mark unread" } else { "Mark read" }}
                    </button>
                    <button class="item-star-action" type="button" aria-pressed=move || item.get().starred.to_string() on:click=move |_| on_star.run(index)>
                        {move || if item.get().starred { "Unstar" } else { "Star" }}
                    </button>
                </div>
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
                id: "one".into(),
                title: "Before".into(),
                source_name: "Example".into(),
                length_label: "5 min".into(),
                time_label: "Now".into(),
                read_state: "unread".into(),
                starred: false,
                primary_url: None,
                summary: None,
                discovered_at_micros: 1,
            };
            let items = RwSignal::new(vec![initial.clone()]);
            let row = row_data(items.into(), 0, initial);
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
        });
    }

    #[test]
    fn item_list_exposes_open_read_and_star_as_native_buttons() {
        let html = Owner::new().with(|| view! {
            <ItemList
                items=Signal::stored(vec![ItemSummary {
                    id: "item-1".into(), title: "The shape of a small reader".into(), source_name: "Example".into(),
                    length_label: "5 min".into(), time_label: "2h ago".into(), read_state: "unread".into(), starred: true,
                    primary_url: None, summary: Some("A compact summary.".into()), discovered_at_micros: 1,
                }])
                selected_index=Signal::stored(0)
                aria_label=String::from("Today")
                on_open=UnsyncCallback::new(|_: usize| {})
                on_read=UnsyncCallback::new(|_: usize| {})
                on_star=UnsyncCallback::new(|_: usize| {})
                on_select=UnsyncCallback::new(|_: usize| {})
            />
        }.to_html());
        assert!(html.contains("The shape of a small reader</button>"));
        assert!(html.contains("Mark read</button>"));
        assert!(html.contains("Unstar</button>"));
        assert!(html.contains("aria-pressed=\"true\""));
        assert!(html.contains("A compact summary."));
    }
}
