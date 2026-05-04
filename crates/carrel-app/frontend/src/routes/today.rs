//! Today list route.

use leptos::prelude::*;

use crate::api::ItemSummary;
#[cfg(target_arch = "wasm32")]
use crate::api::{self, ItemFilter};
use crate::components::TodayList;

/// Items discovered recently.
#[component]
pub fn Today() -> impl IntoView {
    let items = RwSignal::new(Vec::<ItemSummary>::new());
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        Effect::new(move |_| {
            spawn_local(async move {
                match api::list_items(ItemFilter::today()).await {
                    Ok(rows) => {
                        items.set(rows);
                        error.set(None);
                    }
                    Err(err) => error.set(Some(err.message().to_string())),
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
        <section class="route-today">
            <header class="route-header">
                <h1>"Today"</h1>
            </header>
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="quiet-state" aria-hidden="true"></div> }
            >
                {move || match error.get() {
                    Some(message) => view! {
                        <p class="quiet-state">{message}</p>
                    }.into_any(),
                    None if items.get().is_empty() => view! {
                        <p class="quiet-state">"No items today."</p>
                    }.into_any(),
                    None => view! {
                        <TodayList items=items.get()/>
                    }.into_any(),
                }}
            </Show>
        </section>
    }
}
