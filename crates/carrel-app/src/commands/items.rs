//! Item-list commands for the desktop shell.

use std::collections::BTreeMap;
use std::str::FromStr;

use carrel_store::blobs::{BlobId, BlobStore};
use cozo::{DataValue, JsonData, Num};
use kuchiki::traits::TendrilSink;
use serde::{Deserialize, Serialize};
use tauri::State;
use time::{Duration, OffsetDateTime};
use url::Url;

use crate::Result;
use crate::error::AppError;
use crate::state::AppState;

const DEFAULT_ITEM_LIMIT: usize = 100;
const MAX_ITEM_LIMIT: usize = 100;
const TODAY_LOOKBACK: Duration = Duration::hours(24);
const LEGACY_BLOB_URI_PREFIX: &str = "blob://";
const WEBVIEW_BLOB_URI_PREFIX: &str = "carrel-blob://";

/// List items from the local store.
#[tauri::command]
pub fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> Result<Vec<ItemSummary>> {
    list_items_from_store(&state.store, filter)
}

/// Return one item by id, if it exists.
#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: String) -> Result<Option<ItemDetail>> {
    get_item_from_parts(&state.store, &state.blobs, &id)
}

/// Persist reading progress for one item.
#[tauri::command]
pub fn update_read_progress(state: State<'_, AppState>, update: ReadProgressUpdate) -> Result<()> {
    update_read_progress_in_store(&state.store, update)
}

/// Mark one item as read.
#[tauri::command]
pub fn mark_item_read(state: State<'_, AppState>, request: ItemStateRequest) -> Result<()> {
    write_read_state(&state.store, &request.item_id, "read", Some(1.0), None)
}

/// Toggle the starred state for one item and return the new value.
#[tauri::command]
pub fn toggle_item_star(state: State<'_, AppState>, request: ItemStateRequest) -> Result<bool> {
    toggle_star_in_store(&state.store, &request.item_id)
}

/// Item-list filter accepted by the webview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    /// Which list view to query.
    #[serde(default)]
    pub view: ItemView,
    /// Optional row limit. Values above the app maximum are clamped.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Generic item-state mutation request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemStateRequest {
    /// Stable item id.
    pub item_id: String,
}

/// Reading progress update request.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadProgressUpdate {
    /// Stable item id.
    pub item_id: String,
    /// Generic reading progress, clamped from 0.0 to 1.0.
    pub progress: f64,
    /// Scroll offset in CSS pixels for HTML content.
    pub scroll_y: f64,
}

impl Default for ItemFilter {
    fn default() -> Self {
        Self {
            view: ItemView::Today,
            limit: Some(DEFAULT_ITEM_LIMIT),
        }
    }
}

/// Supported item-list views.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemView {
    /// Items discovered in the last day.
    #[default]
    Today,
    /// All items, newest first.
    All,
}

/// Item row rendered by list views.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    /// Stable item id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Source name derived from article metadata or the URL host.
    pub source_name: String,
    /// Human-readable length label.
    pub length_label: String,
    /// Human-readable relative discovery time.
    pub time_label: String,
    /// Local read state, defaulting to unread.
    pub read_state: String,
    /// Canonical URL, when known.
    pub primary_url: Option<String>,
    /// Short summary or feed description, when known.
    pub summary: Option<String>,
    /// Discovery timestamp in Unix microseconds.
    pub discovered_at_micros: i64,
}

/// Fuller item metadata for the reading route.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    /// Stable item id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Source name derived from article metadata or the URL host.
    pub source_name: String,
    /// ISO-8601 published timestamp, when known.
    pub published_at: Option<String>,
    /// Language tag, when known.
    pub language: Option<String>,
    /// Human-readable length label.
    pub length_label: String,
    /// Estimated reading minutes, when known.
    pub estimated_read_minutes: Option<u32>,
    /// Human-readable relative discovery time.
    pub time_label: String,
    /// Local read state, defaulting to unread.
    pub read_state: String,
    /// Whether the item is starred locally.
    pub starred: bool,
    /// Canonical URL, when known.
    pub primary_url: Option<String>,
    /// Short summary or feed description, when known.
    pub summary: Option<String>,
    /// Sanitized readable HTML from the cached content blob.
    pub content_html: String,
    /// Generic creator names from the item relation.
    pub creators: Vec<String>,
    /// Raw article byline, when extracted.
    pub byline: Option<String>,
    /// Blob id for cached readable HTML, when available.
    pub readable_blob_id: Option<String>,
    /// Last scroll offset in CSS pixels, when known.
    pub last_scroll: Option<f64>,
    /// Discovery timestamp in Unix microseconds.
    pub discovered_at_micros: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ItemRecord {
    id: String,
    title: String,
    creators: Vec<String>,
    primary_url: Option<String>,
    published_at_micros: Option<i64>,
    language: Option<String>,
    summary: Option<String>,
    discovered_at_micros: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ArticleMetadata {
    word_count: Option<i64>,
    estimated_read_minutes: Option<i64>,
    site_name: Option<String>,
    byline: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ReadStateRecord {
    state: String,
    progress: Option<f64>,
    last_scroll_y: Option<f64>,
}

pub(crate) fn list_items_from_store(
    store: &carrel_store::Store,
    filter: ItemFilter,
) -> Result<Vec<ItemSummary>> {
    let limit = filter
        .limit
        .unwrap_or(DEFAULT_ITEM_LIMIT)
        .clamp(1, MAX_ITEM_LIMIT);
    let query_limit = match filter.view {
        ItemView::Today => 500,
        ItemView::All => limit,
    };
    let now = OffsetDateTime::now_utc();
    let since = now - TODAY_LOOKBACK;

    let mut rows = Vec::new();
    for item in query_item_records(store, query_limit)? {
        if filter.view == ItemView::Today && item.discovered_at_micros < unix_micros(since) {
            continue;
        }

        let article = article_metadata(store, &item.id)?;
        let read_state = read_state(store, &item.id)?;
        rows.push(summary_from_parts(&item, &article, read_state.state, now));

        if rows.len() == limit {
            break;
        }
    }

    Ok(rows)
}

pub(crate) fn get_item_from_parts(
    store: &carrel_store::Store,
    blobs: &BlobStore,
    id: &str,
) -> Result<Option<ItemDetail>> {
    let Some(item) = query_item_record(store, id)? else {
        return Ok(None);
    };

    let article = article_metadata(store, id)?;
    let read_state = read_state(store, id)?;
    let starred = starred_state(store, id)?;
    let summary = summary_from_parts(
        &item,
        &article,
        read_state.state.clone(),
        OffsetDateTime::now_utc(),
    );
    let readable_blob_id = store
        .get_item_detail(id)?
        .and_then(|detail| detail.readable.map(|readable| readable.blob_id));
    let content_html = readable_blob_id
        .as_deref()
        .map(|blob_id| readable_html(blobs, blob_id))
        .transpose()?
        .filter(|html| !looks_like_network_error_page(html))
        .or_else(|| item.summary.as_deref().map(summary_content_html))
        .map(|html| rewrite_blob_uris_for_webview(&html))
        .unwrap_or_default();

    Ok(Some(ItemDetail {
        id: summary.id,
        title: summary.title,
        source_name: summary.source_name,
        published_at: item.published_at_micros.map(iso8601_timestamp),
        language: item.language,
        length_label: summary.length_label,
        estimated_read_minutes: article
            .estimated_read_minutes
            .and_then(|minutes| u32::try_from(minutes).ok()),
        time_label: summary.time_label,
        read_state: summary.read_state,
        starred,
        primary_url: summary.primary_url,
        summary: summary.summary,
        content_html,
        creators: item.creators,
        byline: article.byline,
        readable_blob_id,
        last_scroll: read_state.last_scroll_y,
        discovered_at_micros: summary.discovered_at_micros,
    }))
}

fn query_item_records(store: &carrel_store::Store, limit: usize) -> Result<Vec<ItemRecord>> {
    let rows = store.query(&format!(
        r#"
        ?[id, title, creators, primary_url, published_at, language, summary, discovered_at] :=
            *item{{id, title, creators, primary_url, published_at, language, summary, discovered_at}}
        :sort -discovered_at
        :limit {limit}
        "#
    ))?;

    rows.rows.iter().map(|row| decode_item_row(row)).collect()
}

fn query_item_record(store: &carrel_store::Store, id: &str) -> Result<Option<ItemRecord>> {
    let rows = store.query_with_params(
        r#"
        ?[id, title, creators, primary_url, published_at, language, summary, discovered_at] :=
            *item{id, title, creators, primary_url, published_at, language, summary, discovered_at},
            id = $id
        :limit 1
        "#,
        BTreeMap::from([("id".to_string(), DataValue::from(id))]),
    )?;

    rows.rows
        .first()
        .map(|row| decode_item_row(row))
        .transpose()
}

fn article_metadata(store: &carrel_store::Store, item_id: &str) -> Result<ArticleMetadata> {
    let rows = store.query_with_params(
        r#"
        ?[word_count, estimated_read_minutes, site_name, byline] :=
            *item_article{item_id, word_count, estimated_read_minutes, site_name, byline},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    let Some(row) = rows.rows.first() else {
        return Ok(ArticleMetadata::default());
    };

    Ok(ArticleMetadata {
        word_count: optional_i64(required(row, 0, "item_article.word_count")?)?,
        estimated_read_minutes: optional_i64(required(
            row,
            1,
            "item_article.estimated_read_minutes",
        )?)?,
        site_name: optional_string(required(row, 2, "item_article.site_name")?)?,
        byline: optional_string(required(row, 3, "item_article.byline")?)?,
    })
}

fn read_state(store: &carrel_store::Store, item_id: &str) -> Result<ReadStateRecord> {
    let rows = store.query_with_params(
        r#"
        ?[state, progress, last_position] :=
            *read_state{item_id, state, progress, last_position},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    let Some(row) = rows.rows.first() else {
        return Ok(ReadStateRecord {
            state: "unread".to_string(),
            progress: None,
            last_scroll_y: None,
        });
    };

    Ok(ReadStateRecord {
        state: value_as_string(required(row, 0, "read_state.state")?)?,
        progress: optional_f64(required(row, 1, "read_state.progress")?)?,
        last_scroll_y: optional_scroll_y(required(row, 2, "read_state.last_position")?)?,
    })
}

fn starred_state(store: &carrel_store::Store, item_id: &str) -> Result<bool> {
    let rows = store.query_with_params(
        r#"
        ?[starred] :=
            *item_star{item_id, starred},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    rows.rows
        .first()
        .map(|row| value_as_bool(required(row, 0, "item_star.starred")?))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn update_read_progress_in_store(
    store: &carrel_store::Store,
    update: ReadProgressUpdate,
) -> Result<()> {
    let current = read_state(store, &update.item_id)?;
    let state = if current.state == "read" {
        "read"
    } else {
        "reading"
    };
    write_read_state(
        store,
        &update.item_id,
        state,
        Some(update.progress.clamp(0.0, 1.0)),
        Some(update.scroll_y.max(0.0)),
    )
}

fn write_read_state(
    store: &carrel_store::Store,
    item_id: &str,
    state: &str,
    progress: Option<f64>,
    scroll_y: Option<f64>,
) -> Result<()> {
    let progress_value = progress
        .map(|value| DataValue::Num(Num::Float(value)))
        .unwrap_or(DataValue::Null);
    let progress_label = progress
        .map(|value| format!("{}%", (value * 100.0).round()))
        .map(DataValue::from)
        .unwrap_or(DataValue::Null);
    let last_position = scroll_y
        .map(|value| DataValue::Json(JsonData(serde_json::json!({ "scroll_y": value }))))
        .unwrap_or(DataValue::Null);

    store.query_with_params(
        r#"
        ?[item_id, state, progress, progress_label, last_position, updated_at] :=
            item_id = $item_id,
            state = $state,
            progress = $progress,
            progress_label = $progress_label,
            last_position = $last_position,
            updated_at = $updated_at
        :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
        "#,
        BTreeMap::from([
            ("item_id".to_string(), DataValue::from(item_id)),
            ("state".to_string(), DataValue::from(state)),
            ("progress".to_string(), progress_value),
            ("progress_label".to_string(), progress_label),
            ("last_position".to_string(), last_position),
            (
                "updated_at".to_string(),
                DataValue::Validity(cozo::Validity::from((
                    unix_micros(OffsetDateTime::now_utc()),
                    true,
                ))),
            ),
        ]),
    )?;

    Ok(())
}

fn toggle_star_in_store(store: &carrel_store::Store, item_id: &str) -> Result<bool> {
    let starred = !starred_state(store, item_id)?;
    store.query_with_params(
        r#"
        ?[item_id, starred, updated_at] :=
            item_id = $item_id,
            starred = $starred,
            updated_at = $updated_at
        :put item_star {item_id => starred, updated_at}
        "#,
        BTreeMap::from([
            ("item_id".to_string(), DataValue::from(item_id)),
            ("starred".to_string(), DataValue::Bool(starred)),
            (
                "updated_at".to_string(),
                DataValue::Validity(cozo::Validity::from((
                    unix_micros(OffsetDateTime::now_utc()),
                    true,
                ))),
            ),
        ]),
    )?;

    Ok(starred)
}

fn readable_html(blobs: &BlobStore, blob_id: &str) -> Result<String> {
    let blob_id = BlobId::from_str(blob_id)?;
    let bytes = blobs.get_blocking(&blob_id)?;
    String::from_utf8(bytes.to_vec()).map_err(|source| AppError::InvalidData {
        context: "item_content.blob",
        value: source.to_string(),
    })
}

fn rewrite_blob_uris_for_webview(html: &str) -> String {
    let document = kuchiki::parse_html().one(html);

    if let Ok(nodes) =
        document.select("img[src], source[src], audio[src], video[src], video[poster]")
    {
        for node in nodes {
            for attr_name in ["src", "poster"] {
                let rewritten = {
                    let attributes = node.attributes.borrow();
                    attributes.get(attr_name).and_then(rewrite_legacy_blob_url)
                };
                if let Some(rewritten) = rewritten {
                    node.attributes.borrow_mut().insert(attr_name, rewritten);
                }
            }
        }
    }

    if let Ok(nodes) = document.select("img[srcset], source[srcset]") {
        for node in nodes {
            let Some(rewritten) = node
                .attributes
                .borrow()
                .get("srcset")
                .and_then(rewrite_legacy_blob_srcset)
            else {
                continue;
            };
            node.attributes.borrow_mut().insert("srcset", rewritten);
        }
    }

    serialize_body_children(&document)
}

fn rewrite_legacy_blob_url(value: &str) -> Option<String> {
    value
        .strip_prefix(LEGACY_BLOB_URI_PREFIX)
        .map(|suffix| format!("{WEBVIEW_BLOB_URI_PREFIX}{suffix}"))
}

fn rewrite_legacy_blob_srcset(value: &str) -> Option<String> {
    let mut changed = false;
    let rewritten = value
        .split(',')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(|candidate| {
            let mut parts = candidate.split_whitespace();
            let url = parts.next().unwrap_or_default();
            let descriptor = parts.collect::<Vec<_>>().join(" ");
            let url = if let Some(rewritten) = rewrite_legacy_blob_url(url) {
                changed = true;
                rewritten
            } else {
                url.to_string()
            };

            if descriptor.is_empty() {
                url
            } else {
                format!("{url} {descriptor}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    changed.then_some(rewritten)
}

fn serialize_body_children(document: &kuchiki::NodeRef) -> String {
    let Ok(body) = document.select_first("body") else {
        return document.to_string();
    };

    body.as_node()
        .children()
        .map(|child| child.to_string())
        .collect()
}

fn iso8601_timestamp(micros: i64) -> String {
    let timestamp = OffsetDateTime::from_unix_timestamp(micros / 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let date = timestamp.date();
    let time = timestamp.time();

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year(),
        u8::from(date.month()),
        date.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

fn summary_from_parts(
    item: &ItemRecord,
    article: &ArticleMetadata,
    read_state: String,
    now: OffsetDateTime,
) -> ItemSummary {
    ItemSummary {
        id: item.id.clone(),
        title: item.title.clone(),
        source_name: source_name(item.primary_url.as_deref(), article.site_name.as_deref()),
        length_label: length_label(article),
        time_label: time_label(item.discovered_at_micros, now),
        read_state,
        primary_url: item.primary_url.clone(),
        summary: item.summary.as_deref().map(plain_text_summary),
        discovered_at_micros: item.discovered_at_micros,
    }
}

fn plain_text_summary(summary: &str) -> String {
    kuchiki::parse_html()
        .one(summary)
        .text_contents()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn summary_content_html(summary: &str) -> String {
    let summary = plain_text_summary(summary);
    if summary.is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>", html_escape::encode_text(&summary))
    }
}

fn looks_like_network_error_page(html: &str) -> bool {
    let text = plain_text_summary(html).to_lowercase().replace('’', "'");
    text.contains("we can't find the internet") && text.contains("attempting to reconnect")
}

fn source_name(primary_url: Option<&str>, site_name: Option<&str>) -> String {
    if let Some(site_name) = site_name.filter(|value| !value.trim().is_empty()) {
        return site_name.to_string();
    }

    primary_url
        .and_then(|value| Url::parse(value).ok())
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "Unknown source".to_string())
}

fn length_label(article: &ArticleMetadata) -> String {
    if let Some(minutes) = article
        .estimated_read_minutes
        .filter(|minutes| *minutes > 0)
    {
        return format!("{minutes} min");
    }

    if let Some(words) = article.word_count.filter(|words| *words > 0) {
        return format!("{words} words");
    }

    "unknown length".to_string()
}

fn time_label(discovered_at_micros: i64, now: OffsetDateTime) -> String {
    let discovered_at =
        OffsetDateTime::from_unix_timestamp(discovered_at_micros / 1_000_000).unwrap_or(now);
    let age = now - discovered_at;

    if age <= Duration::ZERO {
        "now".to_string()
    } else if age < Duration::hours(1) {
        format!("{}m ago", age.whole_minutes().max(1))
    } else if age < Duration::days(1) {
        format!("{}h ago", age.whole_hours())
    } else if age < Duration::days(7) {
        format!("{}d ago", age.whole_days())
    } else {
        let date = discovered_at.date();
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        )
    }
}

fn decode_item_row(row: &[DataValue]) -> Result<ItemRecord> {
    Ok(ItemRecord {
        id: value_as_string(required(row, 0, "item.id")?)?,
        title: value_as_string(required(row, 1, "item.title")?)?,
        creators: value_as_string_list(required(row, 2, "item.creators")?)?,
        primary_url: optional_string(required(row, 3, "item.primary_url")?)?,
        published_at_micros: optional_validity_micros(required(row, 4, "item.published_at")?)?,
        language: optional_string(required(row, 5, "item.language")?)?,
        summary: optional_string(required(row, 6, "item.summary")?)?,
        discovered_at_micros: value_as_validity_micros(required(row, 7, "item.discovered_at")?)?,
    })
}

fn required<'a>(
    row: &'a [DataValue],
    index: usize,
    context: &'static str,
) -> Result<&'a DataValue> {
    row.get(index).ok_or_else(|| AppError::InvalidData {
        context,
        value: "missing column".to_string(),
    })
}

fn value_as_string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        other => Err(unexpected("string", other)),
    }
}

fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Str(value) => Ok(Some(value.to_string())),
        other => Err(unexpected("optional string", other)),
    }
}

fn optional_i64(value: &DataValue) -> Result<Option<i64>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Num(Num::Int(value)) => Ok(Some(*value)),
        other => Err(unexpected("optional integer", other)),
    }
}

fn optional_f64(value: &DataValue) -> Result<Option<f64>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Num(Num::Float(value)) => Ok(Some(*value)),
        DataValue::Num(Num::Int(value)) => Ok(Some(*value as f64)),
        other => Err(unexpected("optional float", other)),
    }
}

fn optional_scroll_y(value: &DataValue) -> Result<Option<f64>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Json(JsonData(value)) => {
            Ok(value.get("scroll_y").and_then(serde_json::Value::as_f64))
        }
        other => Err(unexpected("optional scroll position", other)),
    }
}

fn value_as_bool(value: &DataValue) -> Result<bool> {
    match value {
        DataValue::Bool(value) => Ok(*value),
        other => Err(unexpected("boolean", other)),
    }
}

fn value_as_validity_micros(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Validity(value) => Ok(value.timestamp.0.0),
        other => Err(unexpected("validity", other)),
    }
}

fn optional_validity_micros(value: &DataValue) -> Result<Option<i64>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Validity(value) => Ok(Some(value.timestamp.0.0)),
        other => Err(unexpected("optional validity", other)),
    }
}

fn value_as_string_list(value: &DataValue) -> Result<Vec<String>> {
    match value {
        DataValue::List(values) => values
            .iter()
            .map(value_as_string)
            .collect::<Result<Vec<_>>>(),
        other => Err(unexpected("string list", other)),
    }
}

fn unexpected(context: &'static str, value: &DataValue) -> AppError {
    AppError::InvalidData {
        context,
        value: format!("{value:?}"),
    }
}

fn unix_micros(value: OffsetDateTime) -> i64 {
    i64::try_from(value.unix_timestamp_nanos() / 1_000).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use cozo::Validity;

    use super::*;

    #[test]
    fn today_filter_returns_recent_items_as_dtos() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let now = OffsetDateTime::now_utc();

        insert_item(
            &store,
            InsertItem {
                id: "recent-item",
                title: "A Plain Web Page",
                url: Some("https://example.com/plain"),
                summary: Some("A short summary."),
                discovered_at_micros: unix_micros(now - Duration::hours(2)),
                read_state: Some("reading"),
                site_name: Some("Example"),
                minutes: Some(4),
            },
        );
        insert_item(
            &store,
            InsertItem {
                id: "old-item",
                title: "Yesterday's News",
                url: Some("https://old.example/news"),
                summary: None,
                discovered_at_micros: unix_micros(now - Duration::days(2)),
                read_state: None,
                site_name: None,
                minutes: None,
            },
        );

        let rows = list_items_from_store(&store, ItemFilter::default()).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "recent-item");
        assert_eq!(rows[0].source_name, "Example");
        assert_eq!(rows[0].length_label, "4 min");
        assert_eq!(rows[0].read_state, "reading");
    }

    #[test]
    fn item_summary_uses_plain_text_preview() {
        let item = ItemRecord {
            id: "html-summary".to_string(),
            title: "HTML Summary".to_string(),
            creators: Vec::new(),
            primary_url: None,
            published_at_micros: None,
            language: None,
            summary: Some("<p>Hello <strong>world</strong>&nbsp;today.</p>".to_string()),
            discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
        };

        let summary = summary_from_parts(
            &item,
            &ArticleMetadata::default(),
            "unread".to_string(),
            OffsetDateTime::now_utc(),
        );

        assert_eq!(summary.summary.as_deref(), Some("Hello world today."));
    }

    #[test]
    fn item_detail_includes_creators_and_readable_content() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let body = b"<p>Cached body.</p>";
        let blob_id = blobs.put_blocking(body).unwrap().to_string();

        insert_item(
            &store,
            InsertItem {
                id: "item-with-content",
                title: "Cached Essay",
                url: Some("https://essays.example/read"),
                summary: None,
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: None,
                minutes: Some(12),
            },
        );
        insert_item_content(&store, "item-with-content", blob_id.as_str(), body.len());

        let detail = get_item_from_parts(&store, &blobs, "item-with-content")
            .unwrap()
            .unwrap();

        assert_eq!(detail.title, "Cached Essay");
        assert_eq!(detail.creators, vec!["Ada"]);
        assert_eq!(detail.source_name, "essays.example");
        assert_eq!(detail.readable_blob_id.as_deref(), Some(blob_id.as_str()));
        assert_eq!(detail.content_html, "<p>Cached body.</p>");
    }

    #[test]
    fn item_detail_falls_back_to_summary_for_network_error_body() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let body =
            b"<main><h1>We can't find the internet</h1><p>Attempting to reconnect</p></main>";
        let blob_id = blobs.put_blocking(body).unwrap().to_string();

        insert_item(
            &store,
            InsertItem {
                id: "item-with-network-error-body",
                title: "Cached Essay",
                url: Some("https://essays.example/read"),
                summary: Some("<p>Real feed summary.</p>"),
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: None,
                minutes: Some(12),
            },
        );
        insert_item_content(
            &store,
            "item-with-network-error-body",
            blob_id.as_str(),
            body.len(),
        );

        let detail = get_item_from_parts(&store, &blobs, "item-with-network-error-body")
            .unwrap()
            .unwrap();

        assert_eq!(detail.content_html, "<p>Real feed summary.</p>");
        assert_eq!(detail.readable_blob_id.as_deref(), Some(blob_id.as_str()));
    }

    #[test]
    fn item_detail_rewrites_legacy_blob_urls_for_the_webview_protocol() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let body = b"<p><img src=\"blob://abc\" srcset=\"blob://def 2x\"><video src=\"blob://ghi\" poster=\"blob://jkl\"></video></p>";
        let blob_id = blobs.put_blocking(body).unwrap().to_string();

        insert_item(
            &store,
            InsertItem {
                id: "item-with-legacy-blob-url",
                title: "Cached Essay",
                url: Some("https://essays.example/read"),
                summary: None,
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: None,
                minutes: Some(12),
            },
        );
        insert_item_content(
            &store,
            "item-with-legacy-blob-url",
            blob_id.as_str(),
            body.len(),
        );

        let detail = get_item_from_parts(&store, &blobs, "item-with-legacy-blob-url")
            .unwrap()
            .unwrap();

        assert!(detail.content_html.contains("carrel-blob://abc"));
        assert!(detail.content_html.contains("carrel-blob://def 2x"));
        assert!(detail.content_html.contains("carrel-blob://ghi"));
        assert!(detail.content_html.contains("carrel-blob://jkl"));
        assert!(!detail.content_html.contains("src=\"blob://"));
        assert!(!detail.content_html.contains("srcset=\"blob://"));
        assert!(!detail.content_html.contains("poster=\"blob://"));
        assert_eq!(
            rewrite_blob_uris_for_webview(&detail.content_html),
            detail.content_html
        );
    }

    #[test]
    fn read_progress_persists_scroll_position() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        insert_item(
            &store,
            InsertItem {
                id: "progress-item",
                title: "Progress",
                url: None,
                summary: Some("fallback"),
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: None,
                minutes: None,
            },
        );

        update_read_progress_in_store(
            &store,
            ReadProgressUpdate {
                item_id: "progress-item".to_string(),
                progress: 0.42,
                scroll_y: 640.0,
            },
        )
        .unwrap();

        let record = read_state(&store, "progress-item").unwrap();

        assert_eq!(record.state, "reading");
        assert_eq!(record.progress, Some(0.42));
        assert_eq!(record.last_scroll_y, Some(640.0));
    }

    #[test]
    fn toggle_star_returns_the_new_state() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        insert_item(
            &store,
            InsertItem {
                id: "starred-item",
                title: "Starred",
                url: None,
                summary: None,
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: None,
                minutes: None,
            },
        );

        assert!(toggle_star_in_store(&store, "starred-item").unwrap());
        assert!(starred_state(&store, "starred-item").unwrap());
        assert!(!toggle_star_in_store(&store, "starred-item").unwrap());
        assert!(!starred_state(&store, "starred-item").unwrap());
    }

    struct InsertItem<'a> {
        id: &'a str,
        title: &'a str,
        url: Option<&'a str>,
        summary: Option<&'a str>,
        discovered_at_micros: i64,
        read_state: Option<&'a str>,
        site_name: Option<&'a str>,
        minutes: Option<i64>,
    }

    fn insert_item(store: &carrel_store::Store, item: InsertItem<'_>) {
        store
            .query_with_params(
                r#"
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = $id,
                    kind = 'article',
                    title = $title,
                    creators = $creators,
                    primary_url = $primary_url,
                    published_at = null,
                    language = 'en',
                    summary = $summary,
                    discovered_at = $discovered_at
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
                "#,
                BTreeMap::from([
                    ("id".to_string(), DataValue::from(item.id)),
                    ("title".to_string(), DataValue::from(item.title)),
                    (
                        "creators".to_string(),
                        DataValue::List(vec![DataValue::from("Ada")]),
                    ),
                    (
                        "primary_url".to_string(),
                        item.url.map(DataValue::from).unwrap_or(DataValue::Null),
                    ),
                    (
                        "summary".to_string(),
                        item.summary.map(DataValue::from).unwrap_or(DataValue::Null),
                    ),
                    (
                        "discovered_at".to_string(),
                        DataValue::Validity(Validity::from((item.discovered_at_micros, true))),
                    ),
                ]),
            )
            .unwrap();

        store
            .query_with_params(
                r#"
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = $item_id,
                    feed_url = null,
                    word_count = 1200,
                    estimated_read_minutes = $estimated_read_minutes,
                    site_name = $site_name,
                    byline = 'Ada'
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(item.id)),
                    (
                        "estimated_read_minutes".to_string(),
                        item.minutes
                            .map(|value| DataValue::Num(Num::Int(value)))
                            .unwrap_or(DataValue::Null),
                    ),
                    (
                        "site_name".to_string(),
                        item.site_name.map(DataValue::from).unwrap_or(DataValue::Null),
                    ),
                ]),
            )
            .unwrap();

        if let Some(state) = item.read_state {
            store
                .query_with_params(
                    r#"
                    ?[item_id, state, progress, progress_label, last_position, updated_at] :=
                        item_id = $item_id,
                        state = $state,
                        progress = 0.3,
                        progress_label = null,
                        last_position = null,
                        updated_at = $updated_at
                    :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
                    "#,
                    BTreeMap::from([
                        ("item_id".to_string(), DataValue::from(item.id)),
                        ("state".to_string(), DataValue::from(state)),
                        (
                            "updated_at".to_string(),
                            DataValue::Validity(Validity::from((
                                item.discovered_at_micros + 1,
                                true,
                            ))),
                        ),
                    ]),
                )
                .unwrap();
        }
    }

    fn insert_item_content(
        store: &carrel_store::Store,
        item_id: &str,
        blob_id: &str,
        byte_size: usize,
    ) {
        store
            .query_with_params(
                r#"
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    item_id = $item_id,
                    format = 'html_readable',
                    blob_id = $blob_id,
                    fetched_at = $fetched_at,
                    extracted_with = 'readability',
                    byte_size = $byte_size
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(item_id)),
                    ("blob_id".to_string(), DataValue::from(blob_id)),
                    (
                        "byte_size".to_string(),
                        DataValue::Num(Num::Int(i64::try_from(byte_size).unwrap())),
                    ),
                    (
                        "fetched_at".to_string(),
                        DataValue::Validity(Validity::from((
                            unix_micros(OffsetDateTime::now_utc()),
                            true,
                        ))),
                    ),
                ]),
            )
            .unwrap();
    }
}
