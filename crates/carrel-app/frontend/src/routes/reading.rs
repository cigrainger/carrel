//! Reading route.

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

use carrel_core::keymap::default;
use leptos::html;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::api::ItemDetail;
#[cfg(target_arch = "wasm32")]
use crate::api::{self, ItemStateRequest, ReadProgressUpdate};
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::{ListNavigation, use_list_navigation_context};

/// Full-page article reader.
#[component]
pub fn ReadingView() -> impl IntoView {
    let params = use_params_map();
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let item_id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let item = RwSignal::new(None::<ItemDetail>);
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    let restored_item = RwSignal::new(None::<String>);
    let scroll_ref = NodeRef::<html::Section>::new();
    let navigate = use_navigate();
    let list_navigation = use_list_navigation_context();

    use_keymap_layer(default::reading_layer());
    use_keymap_layer(default::item_action_layer());
    use_keymap_layer(default::reading_extra_layer());

    let page_down_ref = scroll_ref;
    use_action_handler("page-down", move || {
        scroll_by_pages(page_down_ref, 0.85);
    });

    let page_up_ref = scroll_ref;
    use_action_handler("page-up", move || {
        scroll_by_pages(page_up_ref, -0.85);
    });

    let top_ref = scroll_ref;
    use_action_handler("article-top", move || {
        scroll_to(top_ref, 0.0);
    });

    let bottom_ref = scroll_ref;
    use_action_handler("article-bottom", move || {
        scroll_to_bottom(bottom_ref);
    });

    let back_navigate = navigate.clone();
    use_action_handler("go-back", move || {
        navigate_back(list_navigation, back_navigate.clone());
    });
    let back_button_navigate = navigate.clone();

    let next_navigate = navigate.clone();
    use_action_handler("next-item-mark-read", move || {
        navigate_adjacent(
            item,
            list_navigation,
            next_navigate.clone(),
            Direction::Next,
        );
    });

    let previous_navigate = navigate.clone();
    use_action_handler("prev-item-mark-read", move || {
        navigate_adjacent(
            item,
            list_navigation,
            previous_navigate.clone(),
            Direction::Previous,
        );
    });

    use_action_handler("mark-read", move || {
        mark_current_read(item);
    });

    use_action_handler("toggle-star", move || {
        toggle_current_star(item);
    });

    let restore_ref = scroll_ref;
    Effect::new(move |_| {
        let Some(detail) = item.get() else {
            return;
        };
        if restored_item.get_untracked().as_deref() == Some(detail.id.as_str()) {
            return;
        }

        restored_item.set(Some(detail.id));
        restore_scroll(restore_ref, detail.last_scroll);
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        Effect::new(move |_| {
            let id = item_id.get();
            item.set(None);
            error.set(None);
            loaded.set(false);
            restored_item.set(None);

            spawn_local(async move {
                match api::get_item(id).await {
                    Ok(detail) => {
                        item.set(detail);
                    }
                    Err(err) => {
                        error.set(Some(err.message().to_string()));
                    }
                }
                loaded.set(true);
            });
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        loaded.set(true);
    }

    view! {
        <section
            class="reading-route"
            aria-label="Reading view"
            node_ref=scroll_ref
            on:scroll=move |_| persist_progress(item, scroll_ref)
        >
            <button
                type="button"
                class="reader-back"
                on:click=move |_| navigate_back(list_navigation, back_button_navigate.clone())
            >
                "Back"
            </button>
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="reading-empty" aria-hidden="true"></div> }
            >
                {move || match error.get() {
                    Some(message) => view! {
                        <p class="quiet-state reader-message">{message}</p>
                    }.into_any(),
                    None => item.get()
                        .map(|detail| view! { <ArticleContent item=detail/> }.into_any())
                        .unwrap_or_else(|| view! {
                            <p class="quiet-state reader-message">"Item not found."</p>
                        }.into_any()),
                }}
            </Show>
        </section>
    }
}

/// Render the article payload itself.
#[component]
pub fn ArticleContent(item: ItemDetail) -> impl IntoView {
    let byline = item.byline.clone().or_else(|| {
        if item.creators.is_empty() {
            None
        } else {
            Some(item.creators.join(", "))
        }
    });
    let has_byline = byline.is_some();
    let byline_label = byline.unwrap_or_default();
    let date = item.published_at.clone().unwrap_or(item.time_label.clone());
    let star_label = if item.starred { "Starred" } else { "" };
    let read_label = if item.read_state == "read" {
        "Read"
    } else {
        ""
    };
    let has_original = item.primary_url.is_some();

    view! {
        <article class="reader-article" lang=item.language.clone().unwrap_or_else(|| "en".to_string())>
            <header class="reader-header">
                <p class="reader-source">{item.source_name.clone()}</p>
                <h1>{item.title.clone()}</h1>
                <div class="reader-meta">
                    <Show when=move || has_byline>
                        <span>{byline_label.clone()}</span>
                    </Show>
                    <span>{item.length_label.clone()}</span>
                    <span>{date}</span>
                    <Show when=move || !star_label.is_empty()>
                        <span>{star_label}</span>
                    </Show>
                    <Show when=move || !read_label.is_empty()>
                        <span>{read_label}</span>
                    </Show>
                </div>
            </header>
            <div class="reader-body" inner_html=item.content_html.clone()></div>
            <footer class="reader-footer">
                <span class="reader-end">"end of article"</span>
                <Show when=move || has_original>
                    <a
                        href=item.primary_url.clone().unwrap_or_default()
                        target="_blank"
                        rel="noreferrer"
                    >
                        "Open original"
                    </a>
                </Show>
            </footer>
        </article>
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Next,
    Previous,
}

fn navigate_back(
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions),
) {
    let return_path = list_navigation
        .get()
        .map(|navigation| navigation.return_path)
        .unwrap_or_else(|| "/today".to_string());
    navigate(&return_path, NavigateOptions::default());
}

fn navigate_adjacent(
    item: RwSignal<Option<ItemDetail>>,
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions) + Clone + 'static,
    direction: Direction,
) {
    let Some(current_id) =
        item.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()))
    else {
        return;
    };
    let Some(next_navigation) = next_list_navigation(list_navigation.get(), &current_id, direction)
    else {
        return;
    };

    mark_current_read(item);
    let next_id = next_navigation.current_id.clone();
    list_navigation.set(next_navigation);
    navigate(&format!("/item/{next_id}"), NavigateOptions::default());
}

fn next_list_navigation(
    context: Option<ListNavigation>,
    current_id: &str,
    direction: Direction,
) -> Option<ListNavigation> {
    let mut context = context?;
    let current = context
        .item_ids
        .iter()
        .position(|item_id| item_id == current_id)?;
    let next = match direction {
        Direction::Next => current.checked_add(1)?,
        Direction::Previous => current.checked_sub(1)?,
    };
    let next_id = context.item_ids.get(next)?.clone();
    context.current_id = next_id;
    Some(context)
}

fn mark_current_read(item: RwSignal<Option<ItemDetail>>) {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let Some(item_id) =
        item.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()))
    else {
        return;
    };

    item.update(|detail| {
        if let Some(detail) = detail {
            detail.read_state = "read".to_string();
        }
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            let _ = api::mark_item_read(ItemStateRequest { item_id }).await;
        });
    }
}

fn toggle_current_star(item: RwSignal<Option<ItemDetail>>) {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let Some(item_id) =
        item.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()))
    else {
        return;
    };

    item.update(|detail| {
        if let Some(detail) = detail {
            detail.starred = !detail.starred;
        }
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            if let Ok(starred) = api::toggle_item_star(ItemStateRequest { item_id }).await {
                item.update(|detail| {
                    if let Some(detail) = detail {
                        detail.starred = starred;
                    }
                });
            }
        });
    }
}

fn persist_progress(item: RwSignal<Option<ItemDetail>>, scroll_ref: NodeRef<html::Section>) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        let Some(detail) = item.get_untracked() else {
            return;
        };
        let Some(element) = scroll_ref.get_untracked() else {
            return;
        };
        let max_scroll = (element.scroll_height() - element.client_height()).max(0);
        let scroll_y = f64::from(element.scroll_top().max(0));
        let progress = if max_scroll == 0 {
            1.0
        } else {
            (scroll_y / f64::from(max_scroll)).clamp(0.0, 1.0)
        };

        spawn_local(async move {
            let _ = api::update_read_progress(ReadProgressUpdate {
                item_id: detail.id,
                progress,
                scroll_y,
            })
            .await;
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (item, scroll_ref);
    }
}

fn scroll_by_pages(scroll_ref: NodeRef<html::Section>, pages: f64) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        let delta = f64::from(element.client_height()) * pages;
        scroll_to(scroll_ref, f64::from(element.scroll_top()) + delta);
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, pages);
}

fn scroll_to(scroll_ref: NodeRef<html::Section>, scroll_y: f64) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        let max_scroll = f64::from((element.scroll_height() - element.client_height()).max(0));
        element.set_scroll_top(scroll_y.clamp(0.0, max_scroll).round() as i32);
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, scroll_y);
}

fn scroll_to_bottom(scroll_ref: NodeRef<html::Section>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        element.set_scroll_top(element.scroll_height());
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = scroll_ref;
}

fn restore_scroll(scroll_ref: NodeRef<html::Section>, scroll_y: Option<f64>) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(scroll_y) = scroll_y else {
            return;
        };
        let _ = set_timeout_with_handle(
            move || {
                scroll_to(scroll_ref, scroll_y);
            },
            Duration::from_millis(0),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, scroll_y);
}

#[cfg(test)]
mod tests {
    use leptos::prelude::*;

    use super::*;

    #[test]
    fn article_content_renders_cached_html() {
        let item = ItemDetail {
            id: "item-1".to_string(),
            title: "A quiet page".to_string(),
            source_name: "Example".to_string(),
            published_at: Some("2026-04-30T00:00:00Z".to_string()),
            language: Some("en".to_string()),
            length_label: "5 min".to_string(),
            estimated_read_minutes: Some(5),
            time_label: "2d ago".to_string(),
            read_state: "unread".to_string(),
            starred: true,
            primary_url: Some("https://example.com/page".to_string()),
            summary: None,
            content_html: "<p>First paragraph.</p><pre><code>let x = 1;</code></pre>".to_string(),
            creators: vec!["Ada".to_string()],
            byline: None,
            readable_blob_id: None,
            last_scroll: Some(12.0),
            discovered_at_micros: 1,
        };

        let html = Owner::new().with(|| view! { <ArticleContent item=item/> }.to_html());

        assert!(html.contains("<h1>A quiet page</h1>"));
        assert!(html.contains("<p>First paragraph.</p>"));
        assert!(html.contains("<pre><code>let x = 1;</code></pre>"));
        assert!(html.contains("Starred"));
    }
}
