//! One keyboard surface for working commands, sources and the local archive.

use carrel_core::keymap::palette::fuzzy_score;
#[cfg(target_arch = "wasm32")]
use carrel_core::library::ItemFilter;
use leptos::{html, prelude::*};
use leptos_router::hooks::use_navigate;

use super::provider::use_keymap_context;
use crate::{api, components::Modal, feeds, navigation::ListNavigation, requests::Requests};

#[derive(Clone, PartialEq)]
enum Target {
    Action(String),
    Source(String),
    Article(String),
    ReturnToDialog,
}

#[derive(Clone, PartialEq)]
struct ResultRow {
    label: String,
    detail: String,
    target: Target,
}

#[component]
pub(crate) fn CommandPalette() -> impl IntoView {
    let context = use_keymap_context();
    view! { <Show when=move || context.palette_open.get()><PaletteDialog/></Show> }
}

#[component]
fn PaletteDialog() -> impl IntoView {
    let context = use_keymap_context();
    let controller = feeds::use_feed_controller();
    let navigation = crate::navigation::use_list_navigation_context();
    let navigate = use_navigate();
    let query = RwSignal::new(String::new());
    let articles = RwSignal::new(Vec::<api::ItemSummary>::new());
    let error = RwSignal::new(None::<String>);
    let selected = RwSignal::new(0usize);
    let input = NodeRef::<html::Input>::new();
    let requests = StoredValue::new(Requests::default());
    on_cleanup(move || requests.get_value().invalidate());
    let commands = context.available_layers();
    let commands = StoredValue::new(commands);
    #[cfg(target_arch = "wasm32")]
    let nested = web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| {
            document
                .query_selector("dialog[open]")
                .ok()
                .flatten()
                .is_some()
        });
    #[cfg(not(target_arch = "wasm32"))]
    let nested = false;
    let results = Memo::new(move |_| {
        let query = query.get();
        let mut ranked = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for layer in commands.get_value() {
            for binding in layer.bindings() {
                let action = binding.action.name();
                if nested && action != "show-keymap-help" {
                    continue;
                }
                if action == "open-command-palette"
                    || action.starts_with("close-")
                    || !seen.insert(action.to_string())
                {
                    continue;
                }
                if let Some(score) = fuzzy_score(&query, &binding.description) {
                    ranked.push((
                        score,
                        ResultRow {
                            label: binding.description.clone(),
                            detail: format!("Command · {}", binding.sequence),
                            target: Target::Action(action.into()),
                        },
                    ));
                }
            }
        }
        for feed in controller.feeds().get().into_iter().filter(|_| !nested) {
            if let Some(score) = fuzzy_score(&query, &feed.display_name) {
                ranked.push((
                    score,
                    ResultRow {
                        label: feed.display_name,
                        detail: "Source".into(),
                        target: Target::Source(feed.url),
                    },
                ));
            }
        }
        ranked.sort_by_key(|(score, _)| *score);
        let mut rows: Vec<_> = ranked.into_iter().take(12).map(|(_, row)| row).collect();
        if nested {
            rows.insert(
                0,
                ResultRow {
                    label: "Return to current dialog".into(),
                    detail: "Keep your draft".into(),
                    target: Target::ReturnToDialog,
                },
            );
        }
        rows.extend(
            articles
                .get()
                .into_iter()
                .filter(|_| !nested)
                .map(|article| ResultRow {
                    label: article.title,
                    detail: format!("Article · {}", article.source_name),
                    target: Target::Article(article.id),
                }),
        );
        rows
    });

    Effect::new(move |_| {
        let text = query.get();
        let guard = requests.get_value();
        let generation = guard.begin();
        articles.set(Vec::new());
        error.set(None);
        if nested || text.trim().is_empty() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        set_timeout(
            move || {
                if !guard.accepts(generation) {
                    return;
                }
                let filter = ItemFilter {
                    search: Some(text),
                    limit: Some(8),
                    ..ItemFilter::library()
                };
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::list_items(filter).await;
                    if !guard.accepts(generation) {
                        return;
                    }
                    match result {
                        Ok(page) => {
                            articles.try_set(page.items);
                        }
                        Err(_) => {
                            error.try_set(Some("Article search is unavailable. Commands and sources are still available.".into()));
                        }
                    }
                });
            },
            std::time::Duration::from_millis(150),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (guard, generation);
    });

    let activate = UnsyncCallback::new(move |index: usize| {
        let Some(row) = results.get_untracked().get(index).cloned() else {
            return;
        };
        let navigate = navigate.clone();
        let article_ids = articles
            .get_untracked()
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let return_path = format!(
            "/library?q={}",
            feeds::percent_encode(&query.get_untracked())
        );
        context.palette_open.set(false);
        set_timeout(
            move || match row.target {
                Target::ReturnToDialog => {}
                Target::Action(action) => {
                    #[cfg(target_arch = "wasm32")]
                    super::provider::dispatch_action(
                        context,
                        carrel_core::keymap::Action::named(action),
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = action;
                }
                Target::Source(url) => navigate(&feeds::feed_href(&url), Default::default()),
                Target::Article(id) => {
                    navigation.set(ListNavigation {
                        item_ids: article_ids,
                        current_id: id.clone(),
                        return_path,
                        return_label: "Library".into(),
                    });
                    navigate(&format!("/item/{id}"), Default::default());
                }
            },
            std::time::Duration::ZERO,
        );
    });

    view! {
        <Modal label="Command palette" class="command-palette" on_cancel=UnsyncCallback::new(move |()| context.palette_open.set(false))>
            <label for="palette-query">"Go to an article, source or command"</label>
            <input node_ref=input id="palette-query" autofocus=true autocomplete="off" type="text" role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls="palette-results" aria-activedescendant=move || (!results.get().is_empty()).then(|| format!("palette-option-{}", selected.get().min(results.get().len().saturating_sub(1))))
                placeholder="Search your library…" prop:value=move || query.get()
                on:input=move |event| { selected.set(0); query.set(event_target_value(&event)); }
                on:keydown=move |event| {
                    if event.is_composing() { return; }
                    let length = results.get_untracked().len();
                    match event.key().as_str() {
                        "ArrowDown" => { event.prevent_default(); event.stop_propagation(); selected.update(|index| *index = (*index + 1).min(length.saturating_sub(1))); }
                        "ArrowUp" => { event.prevent_default(); event.stop_propagation(); selected.update(|index| *index = index.saturating_sub(1).min(length.saturating_sub(1))); }
                        "Enter" if !event.is_composing() => { event.prevent_default(); event.stop_propagation(); activate.run(selected.get_untracked().min(length.saturating_sub(1))); }
                        _ => {}
                    }
                    #[cfg(target_arch = "wasm32")]
                    if matches!(event.key().as_str(), "ArrowDown" | "ArrowUp") {
                        let index = selected.get_untracked();
                        set_timeout(move || {
                            if let Some(element) = web_sys::window().and_then(|window| window.document()).and_then(|document| document.get_element_by_id(&format!("palette-option-{index}"))) {
                                let options = web_sys::ScrollIntoViewOptions::new(); options.set_block(web_sys::ScrollLogicalPosition::Nearest); element.scroll_into_view_with_scroll_into_view_options(&options);
                            }
                        }, std::time::Duration::ZERO);
                    }
                }
            />
            <p class="palette-hint">{if nested { "Finish or close the current dialog to navigate. Escape returns to it with your draft intact." } else { "Commands and sources match abbreviations. Articles search titles and cached text." }}</p>
            <div id="palette-results" role="listbox" aria-label="Search results">
                {move || results.get().into_iter().enumerate().map(|(index, row)| view! {
                    <button type="button" role="option" tabindex="-1" id=format!("palette-option-{index}") aria-selected=move || (selected.get() == index).to_string() class:is-selected=move || selected.get() == index
                        on:mousedown=move |event| event.prevent_default() on:click=move |_| activate.run(index)>
                        <span>{row.label}</span><small>{row.detail}</small>
                    </button>
                }).collect_view()}
                <Show when=move || results.get().is_empty()><p class="palette-empty">"No matching articles, sources or commands."</p></Show>
            </div>
            {move || error.get().map(|message| view! { <p class="palette-hint" role="status">{message}</p> })}
            <footer><span>"↑ ↓ to choose · Return to open"</span><button type="button" on:click=move |_| context.palette_open.set(false)>"Close"</button></footer>
        </Modal>
    }
}
