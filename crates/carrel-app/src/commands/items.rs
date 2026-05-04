//! Item-list commands for the desktop shell.

use std::collections::BTreeMap;

use cozo::{DataValue, Num};
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

/// List items from the local store.
#[tauri::command]
pub fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> Result<Vec<ItemSummary>> {
    list_items_from_store(&state.store, filter)
}

/// Return one item by id, if it exists.
#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: String) -> Result<Option<ItemDetail>> {
    get_item_from_store(&state.store, &id)
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
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
    /// Generic creator names from the item relation.
    pub creators: Vec<String>,
    /// Raw article byline, when extracted.
    pub byline: Option<String>,
    /// Blob id for cached readable HTML, when available.
    pub readable_blob_id: Option<String>,
    /// Discovery timestamp in Unix microseconds.
    pub discovered_at_micros: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ItemRecord {
    id: String,
    title: String,
    creators: Vec<String>,
    primary_url: Option<String>,
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
        rows.push(summary_from_parts(&item, &article, read_state, now));

        if rows.len() == limit {
            break;
        }
    }

    Ok(rows)
}

pub(crate) fn get_item_from_store(
    store: &carrel_store::Store,
    id: &str,
) -> Result<Option<ItemDetail>> {
    let Some(item) = query_item_record(store, id)? else {
        return Ok(None);
    };

    let article = article_metadata(store, id)?;
    let read_state = read_state(store, id)?;
    let summary = summary_from_parts(&item, &article, read_state, OffsetDateTime::now_utc());
    let readable_blob_id = store
        .get_item_detail(id)?
        .and_then(|detail| detail.readable.map(|readable| readable.blob_id));

    Ok(Some(ItemDetail {
        id: summary.id,
        title: summary.title,
        source_name: summary.source_name,
        length_label: summary.length_label,
        time_label: summary.time_label,
        read_state: summary.read_state,
        primary_url: summary.primary_url,
        summary: summary.summary,
        creators: item.creators,
        byline: article.byline,
        readable_blob_id,
        discovered_at_micros: summary.discovered_at_micros,
    }))
}

fn query_item_records(store: &carrel_store::Store, limit: usize) -> Result<Vec<ItemRecord>> {
    let rows = store.query(&format!(
        r#"
        ?[id, title, creators, primary_url, summary, discovered_at] :=
            *item{{id, title, creators, primary_url, summary, discovered_at}}
        :sort -discovered_at
        :limit {limit}
        "#
    ))?;

    rows.rows.iter().map(|row| decode_item_row(row)).collect()
}

fn query_item_record(store: &carrel_store::Store, id: &str) -> Result<Option<ItemRecord>> {
    let rows = store.query_with_params(
        r#"
        ?[id, title, creators, primary_url, summary, discovered_at] :=
            *item{id, title, creators, primary_url, summary, discovered_at},
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

fn read_state(store: &carrel_store::Store, item_id: &str) -> Result<String> {
    let rows = store.query_with_params(
        r#"
        ?[state] :=
            *read_state{item_id, state},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    rows.rows
        .first()
        .map(|row| value_as_string(required(row, 0, "read_state.state")?))
        .transpose()
        .map(|state| state.unwrap_or_else(|| "unread".to_string()))
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
        summary: item.summary.clone(),
        discovered_at_micros: item.discovered_at_micros,
    }
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
        summary: optional_string(required(row, 4, "item.summary")?)?,
        discovered_at_micros: value_as_validity_micros(required(row, 5, "item.discovered_at")?)?,
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

fn value_as_validity_micros(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Validity(value) => Ok(value.timestamp.0.0),
        other => Err(unexpected("validity", other)),
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
    fn item_detail_includes_creators_and_readable_content() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
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
        store
            .query_with_params(
                r#"
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    item_id = $item_id,
                    format = 'html_readable',
                    blob_id = 'abc123',
                    fetched_at = $fetched_at,
                    extracted_with = 'readability',
                    byte_size = 42
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from("item-with-content")),
                    (
                        "fetched_at".to_string(),
                        DataValue::Validity(Validity::from((unix_micros(OffsetDateTime::now_utc()), true))),
                    ),
                ]),
            )
            .unwrap();

        let detail = get_item_from_store(&store, "item-with-content")
            .unwrap()
            .unwrap();

        assert_eq!(detail.title, "Cached Essay");
        assert_eq!(detail.creators, vec!["Ada"]);
        assert_eq!(detail.source_name, "essays.example");
        assert_eq!(detail.readable_blob_id.as_deref(), Some("abc123"));
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
                        progress = 30,
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
                            DataValue::Validity(Validity::from((item.discovered_at_micros + 1, true))),
                        ),
                    ]),
                )
                .unwrap();
        }
    }
}
