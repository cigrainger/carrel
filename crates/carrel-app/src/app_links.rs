//! Native URL delivery is bounded, read-only and independent of webview startup.

use std::sync::Mutex;

use carrel_core::app_link::{AppLink, LinkId};
use carrel_store::Store;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::{AppError, Result, state::AppState};

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingLink {
    Target(AppLink),
    Invalid,
}

/// Only the most recently requested destination waits for the webview.
#[derive(Default)]
pub(crate) struct AppLinkInbox(Mutex<Option<PendingLink>>);

impl AppLinkInbox {
    fn receive(&self, value: &str) {
        let link = value
            .parse()
            .map_or(PendingLink::Invalid, PendingLink::Target);
        *self.0.lock().expect("app-link inbox lock") = Some(link);
    }

    fn take(&self) -> Option<PendingLink> {
        self.0.lock().expect("app-link inbox lock").take()
    }
}

/// A resolved allow-listed route, or a quiet non-navigating result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LinkResolution {
    Open { path: String },
    MissingItem,
    MissingHighlight,
    Invalid,
}

pub(crate) fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn receive(app: &AppHandle, value: &str) {
    app.state::<AppLinkInbox>().receive(value);
    focus_main(app);
    if let Some(window) = app.get_webview_window("main") {
        // The URL is never interpolated into executable JavaScript. If startup
        // is not ready, the frontend's initial inbox read will still find it.
        let _ = window.eval("window.dispatchEvent(new Event('carrel-link-ready'))");
    }
}

/// Install native delivery before querying startup URLs to cover either order.
pub(crate) fn install(app: &AppHandle) -> Result<()> {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            receive(&handle, url.as_str());
        }
    });
    if let Some(urls) = app.deep_link().get_current()? {
        for url in urls {
            receive(app, url.as_str());
        }
    }
    // macOS associations belong to the installed bundle. Linux AppImages may
    // need desktop integration; development builds must not replace it.
    #[cfg(all(target_os = "linux", not(debug_assertions)))]
    if let Err(error) = app.deep_link().register_all() {
        tracing::warn!(%error, "Could not register Carrel URL scheme");
    }
    Ok(())
}

/// Consume one pending address and resolve it without reading cached bodies.
#[tauri::command]
pub(crate) async fn take_app_link(
    inbox: State<'_, AppLinkInbox>,
    state: State<'_, AppState>,
) -> Result<Option<LinkResolution>> {
    let Some(link) = inbox.take() else {
        return Ok(None);
    };
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || resolve(&store, link).map(Some))
        .await
        .map_err(|source| AppError::BackgroundTask {
            operation: "resolve app link",
            source,
        })?
}

fn resolve(store: &Store, pending: PendingLink) -> Result<LinkResolution> {
    let PendingLink::Target(link) = pending else {
        return Ok(LinkResolution::Invalid);
    };
    let path = match link {
        AppLink::View(view) => view.path().to_string(),
        AppLink::Item(id) => {
            if !store.contains_item(id.as_str())? {
                return Ok(LinkResolution::MissingItem);
            }
            format!("/item/{id}")
        }
        AppLink::Highlight(id) => {
            let Some(highlight) = store.get_highlight(id.as_str())? else {
                return Ok(LinkResolution::MissingHighlight);
            };
            let Ok(item) = LinkId::new(&highlight.item_id) else {
                return Ok(LinkResolution::Invalid);
            };
            if !store.contains_item(item.as_str())? {
                return Ok(LinkResolution::MissingItem);
            }
            format!("/item/{item}#highlight-{id}")
        }
    };
    Ok(LinkResolution::Open { path })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
    use carrel_core::identity::Keypair;
    use carrel_store::highlights::PutHighlightInput;

    use super::*;

    #[test]
    fn startup_and_running_deliveries_share_one_latest_only_inbox() {
        let inbox = AppLinkInbox::default();
        assert_eq!(inbox.take(), None);
        for _ in 0..1000 {
            inbox.receive("carrel://today");
        }
        inbox.receive("carrel://library");
        assert_eq!(
            inbox.take(),
            Some(PendingLink::Target("carrel://library".parse().unwrap()))
        );
        assert_eq!(inbox.take(), None);
        inbox.receive("file:///tmp/secret");
        assert_eq!(inbox.take(), Some(PendingLink::Invalid));
        inbox.receive("carrel://library");
        assert!(matches!(inbox.take(), Some(PendingLink::Target(_))));
    }

    fn archive() -> (Store, String) {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let id = "12".repeat(32);
        store.query_with_params(r#"
            ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [[$id, 'article', 'Saved article', [], null, null, null, null, [1600000000000000, true]]]
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
        "#, BTreeMap::from([("id".into(), cozo::DataValue::from(id.clone()))])).unwrap();
        (store, id)
    }

    #[test]
    fn item_links_do_not_require_cache_and_missing_or_removed_items_cannot_open() {
        let (store, id) = archive();
        let target = PendingLink::Target(AppLink::item(&id).unwrap());
        let before = store.get_item_detail(&id).unwrap();
        assert_eq!(
            resolve(&store, target.clone()).unwrap(),
            LinkResolution::Open {
                path: format!("/item/{id}")
            }
        );
        assert_eq!(store.get_item_detail(&id).unwrap(), before);
        assert_eq!(
            resolve(
                &store,
                PendingLink::Target(AppLink::item(&"00".repeat(32)).unwrap())
            )
            .unwrap(),
            LinkResolution::MissingItem
        );
        store.query_with_params("?[id, tombstoned_at, reason] <- [[$id, [1700000000000000, true], null]]\n:put entity_tombstone {id, tombstoned_at => reason}", BTreeMap::from([("id".into(), cozo::DataValue::from(id))])).unwrap();
        assert_eq!(
            resolve(&store, target).unwrap(),
            LinkResolution::MissingItem
        );
    }

    #[test]
    fn highlight_links_resolve_the_immutable_target_and_respect_removal() {
        let (store, item) = archive();
        let key = Keypair::from_seed(&[31; 32]);
        let highlight = store
            .put_highlight(
                PutHighlightInput {
                    item_id: item.clone(),
                    location: HighlightLocation::HtmlText(HtmlTextSelector {
                        content_blob_id: None,
                        exact: "passage".into(),
                        prefix: String::new(),
                        suffix: String::new(),
                        start: 0,
                        end: 7,
                    }),
                    location_label: None,
                    color: None,
                    created_at_micros: Some(1700000000000000),
                },
                &key,
            )
            .unwrap();
        let target = PendingLink::Target(AppLink::highlight(&highlight.id).unwrap());
        assert_eq!(
            resolve(&store, target.clone()).unwrap(),
            LinkResolution::Open {
                path: format!("/item/{item}#highlight-{}", highlight.id)
            }
        );
        store.remove_highlight(&highlight.id, &key).unwrap();
        assert_eq!(
            resolve(&store, target).unwrap(),
            LinkResolution::MissingHighlight
        );
        assert_eq!(
            resolve(
                &store,
                PendingLink::Target(AppLink::highlight(&"00".repeat(32)).unwrap())
            )
            .unwrap(),
            LinkResolution::MissingHighlight
        );
    }

    #[test]
    fn invalid_links_and_fixed_views_do_not_query_an_archive() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            resolve(&store, PendingLink::Invalid).unwrap(),
            LinkResolution::Invalid
        );
        assert_eq!(
            resolve(
                &store,
                PendingLink::Target("carrel://library".parse().unwrap())
            )
            .unwrap(),
            LinkResolution::Open {
                path: "/library".into()
            }
        );
    }
}
