//! Item-list row rendering.

use leptos::prelude::*;

use crate::api::ItemSummary;

/// Render a list of items.
#[component]
pub fn TodayList(
    items: Vec<ItemSummary>,
    selected_index: usize,
    on_open: UnsyncCallback<usize>,
) -> impl IntoView {
    let indexed_items = items.into_iter().enumerate().collect::<Vec<_>>();

    view! {
        <ul class="item-list" aria-label="Today">
            <For
                each=move || indexed_items.clone()
                key=|(_, item)| item.id.clone()
                let:row
            >
                {
                    let (index, item) = row;
                    view! { <ItemRow index=index item=item selected=index == selected_index on_open=on_open/> }
                }
            </For>
        </ul>
    }
}

/// One item row in a list route.
#[component]
pub fn ItemRow(
    index: usize,
    item: ItemSummary,
    selected: bool,
    on_open: UnsyncCallback<usize>,
) -> impl IntoView {
    let indicator = if item.read_state == "unread" {
        "●"
    } else {
        "○"
    };
    let excerpt = item.summary.unwrap_or_default();
    let class = if selected {
        "item-row is-selected"
    } else {
        "item-row"
    };

    view! {
        <li
            class=class
            aria-selected=selected.to_string()
            on:click=move |_| on_open.run(index)
        >
            <span class="read-indicator" aria-hidden="true">{indicator}</span>
            <div class="item-row-body">
                <div class="item-row-topline">
                    <h2>{item.title}</h2>
                    <p class="item-meta">
                        <span>{item.source_name}</span>
                        <span>{item.length_label}</span>
                        <span>{item.time_label}</span>
                    </p>
                </div>
                <p class="item-excerpt">{excerpt}</p>
            </div>
        </li>
    }
}

#[cfg(test)]
mod tests {
    use leptos::prelude::*;

    use super::*;

    #[test]
    fn today_list_renders_item_rows() {
        let items = vec![ItemSummary {
            id: "item-1".to_string(),
            title: "The shape of a small reader".to_string(),
            source_name: "Example".to_string(),
            length_label: "5 min".to_string(),
            time_label: "2h ago".to_string(),
            read_state: "unread".to_string(),
            primary_url: Some("https://example.com/item".to_string()),
            summary: Some("A compact summary.".to_string()),
            discovered_at_micros: 1,
        }];

        let html = Owner::new().with(|| {
            view! {
                <TodayList
                    items=items
                    selected_index=0
                    on_open=UnsyncCallback::new(|_: usize| {})
                />
            }
            .to_html()
        });
        let expected = r#"<ul aria-label="Today" class="item-list"> <li aria-selected="true" class="item-row is-selected"><span aria-hidden="true" class="read-indicator">●</span><div class="item-row-body"><div class="item-row-topline"><h2>The shape of a small reader</h2><p class="item-meta"><span>Example</span><span>5 min</span><span>2h ago</span></p></div><p class="item-excerpt">A compact summary.</p></div></li><!></ul>"#;

        assert_eq!(html, expected);
    }
}
