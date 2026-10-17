//! First-run eligibility reads the whole local archive without changing it.

use carrel_core::library::ItemFilter;
use carrel_store::Store;
use tauri::State;

use crate::{AppError, Result, state::AppState};

/// Offer first-run help only when neither articles nor subscriptions exist.
#[tauri::command]
pub(crate) async fn welcome_is_needed(state: State<'_, AppState>) -> Result<bool> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || needs_welcome(&store))
        .await
        .map_err(|source| AppError::BackgroundTask {
            operation: "check first launch",
            source,
        })?
}

fn needs_welcome(store: &Store) -> Result<bool> {
    if !store.list_feeds()?.is_empty() {
        return Ok(false);
    }
    let mut filter = ItemFilter::library();
    filter.limit = Some(1);
    Ok(store.list_item_page(&filter, 0)?.items.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn an_empty_archive_is_eligible_but_a_subscription_alone_is_not() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        assert!(needs_welcome(&store).unwrap());
        store.add_feed("https://example.com/feed").unwrap();
        assert!(!needs_welcome(&store).unwrap());
    }

    #[test]
    fn old_saved_articles_prevent_first_run_help_even_without_subscriptions() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.query_with_params(r#"
            ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [['old-article', 'article', 'An older saved article', [], null, null, null, null, [1600000000000000, true]]]
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
        "#, BTreeMap::new()).unwrap();
        let before = store.get_item_detail("old-article").unwrap();
        assert!(!needs_welcome(&store).unwrap());
        assert_eq!(store.get_item_detail("old-article").unwrap(), before);
    }

    #[test]
    fn a_failed_archive_read_is_not_treated_as_an_empty_installation() {
        let store = Store::open_in_memory().unwrap();
        assert!(needs_welcome(&store).is_err());
    }
}
