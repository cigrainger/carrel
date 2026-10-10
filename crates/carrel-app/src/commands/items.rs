//! Item-list commands for the desktop shell.

use std::collections::BTreeMap;
use std::str::FromStr;

use carrel_core::identity::Keypair;
use carrel_core::library::{ItemFilter, ItemPage, TODAY_WINDOW_MICROS};
use carrel_core::private_state::{ReadIntent, StateKey, StateKind, StateValue};
use carrel_core::shape::Shape;
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

const TODAY_LOOKBACK: Duration = Duration::microseconds(TODAY_WINDOW_MICROS);
const LEGACY_BLOB_URI_PREFIX: &str = "blob://";
const WEBVIEW_BLOB_URI_PREFIX: &str = "carrel-blob://";

/// List items from the local store.
#[tauri::command]
pub fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> Result<ItemPage<ItemSummary>> {
    list_item_page_from_store(&state.store, filter)
}

/// Return one item by id, if it exists.
#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: String) -> Result<Option<ItemDetail>> {
    get_item_from_parts(&state.store, &state.blobs, &id)
}

/// Persist reading progress for one item.
#[tauri::command]
pub fn update_read_progress(state: State<'_, AppState>, update: ReadProgressUpdate) -> Result<()> {
    let device = super::highlights::load_device_key(&state.paths)?;
    state.store.record_read_progress(
        &update.item_id,
        update.progress,
        update.scroll_y,
        unix_micros(OffsetDateTime::now_utc()),
        &device,
    )?;
    Ok(())
}

/// Mark one item as read.
#[tauri::command]
pub fn mark_item_read(state: State<'_, AppState>, request: ItemStateRequest) -> Result<()> {
    let device = super::highlights::load_device_key(&state.paths)?;
    write_read_intent(&state.store, &request.item_id, ReadIntent::Read, &device)
}

/// Mark one item as unread.
#[tauri::command]
pub fn mark_item_unread(state: State<'_, AppState>, request: ItemStateRequest) -> Result<()> {
    let device = super::highlights::load_device_key(&state.paths)?;
    write_read_intent(&state.store, &request.item_id, ReadIntent::Unread, &device)
}

/// Save the explicit star intent displayed when the user acted.
#[tauri::command]
pub fn set_item_star(state: State<'_, AppState>, item_id: String, starred: bool) -> Result<bool> {
    let device = super::highlights::load_device_key(&state.paths)?;
    state.store.write_private_state(
        carrel_core::private_state::StateKey {
            kind: carrel_core::private_state::StateKind::Star,
            target: item_id,
        },
        carrel_core::private_state::StateValue::Star(starred),
        unix_micros(OffsetDateTime::now_utc()),
        &device,
    )?;
    Ok(starred)
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
    /// Whether the item is starred locally.
    pub starred: bool,
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
    /// Feed URL that discovered the item, when known.
    pub feed_url: Option<String>,
    /// ISO-8601 published timestamp, when known.
    pub published_at: Option<String>,
    /// Language tag, when known.
    pub language: Option<String>,
    /// Human-readable length label.
    pub length_label: String,
    /// Estimated reading minutes, when known.
    pub estimated_read_minutes: Option<u32>,
    /// Word count from article extraction, when known.
    pub word_count: Option<u32>,
    /// Human-readable relative discovery time.
    pub time_label: String,
    /// ISO-8601 discovery timestamp.
    pub discovered_at: String,
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
    /// Structural content-shape labels, when detected.
    pub shape_tags: Vec<String>,
    /// Last scroll offset in CSS pixels, when known.
    pub last_scroll: Option<f64>,
    /// Saved reading fraction, used when restoring without resetting progress.
    pub read_progress: Option<f64>,
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
    feed_url: Option<String>,
    word_count: Option<i64>,
    estimated_read_minutes: Option<i64>,
    site_name: Option<String>,
    byline: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ReadStateRecord {
    #[cfg(test)]
    exists: bool,
    state: String,
    progress: Option<f64>,
    last_scroll_y: Option<f64>,
}

pub(crate) fn list_item_page_from_store(
    store: &carrel_store::Store,
    filter: ItemFilter,
) -> Result<ItemPage<ItemSummary>> {
    let now = OffsetDateTime::now_utc();
    let page = store.list_item_page(&filter, unix_micros(now - TODAY_LOOKBACK))?;
    Ok(ItemPage {
        items: page
            .items
            .into_iter()
            .map(|item| {
                let (title, summary) = item_display_text(
                    &item.title,
                    item.primary_url.as_deref(),
                    item.summary.as_deref(),
                );
                ItemSummary {
                    source_name: source_name(
                        item.primary_url.as_deref(),
                        item.site_name.as_deref(),
                    ),
                    length_label: length_label(&ArticleMetadata {
                        estimated_read_minutes: item.estimated_read_minutes,
                        word_count: item.word_count,
                        ..Default::default()
                    }),
                    time_label: time_label(
                        item.published_at_micros
                            .unwrap_or(item.discovered_at_micros),
                        now,
                    ),
                    summary,
                    id: item.id,
                    title,
                    read_state: item.read_state,
                    starred: item.starred,
                    primary_url: item.primary_url,
                    discovered_at_micros: item.discovered_at_micros,
                }
            })
            .collect(),
        next_cursor: page.next_cursor,
    })
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
    let shape_tags = shape_tags(store, id)?;
    let summary = summary_from_parts(
        &item,
        &article,
        read_state.state.clone(),
        starred,
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
        feed_url: article.feed_url.clone(),
        published_at: item.published_at_micros.map(iso8601_timestamp),
        language: item.language,
        length_label: summary.length_label,
        estimated_read_minutes: article
            .estimated_read_minutes
            .and_then(|minutes| u32::try_from(minutes).ok()),
        word_count: article
            .word_count
            .and_then(|words| u32::try_from(words).ok()),
        time_label: summary.time_label,
        discovered_at: iso8601_timestamp(summary.discovered_at_micros),
        read_state: summary.read_state,
        starred,
        primary_url: summary.primary_url,
        summary: summary.summary,
        content_html,
        creators: item.creators,
        byline: article.byline,
        readable_blob_id,
        shape_tags,
        last_scroll: read_state.last_scroll_y,
        read_progress: read_state.progress,
        discovered_at_micros: summary.discovered_at_micros,
    }))
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
        ?[feed_url, word_count, estimated_read_minutes, site_name, byline] :=
            *item_article{item_id, feed_url, word_count, estimated_read_minutes, site_name, byline},
            item_id = $item_id
        :limit 1
        "#,
        BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
    )?;

    let Some(row) = rows.rows.first() else {
        return Ok(ArticleMetadata::default());
    };

    Ok(ArticleMetadata {
        feed_url: optional_string(required(row, 0, "item_article.feed_url")?)?,
        word_count: optional_i64(required(row, 1, "item_article.word_count")?)?,
        estimated_read_minutes: optional_i64(required(
            row,
            2,
            "item_article.estimated_read_minutes",
        )?)?,
        site_name: optional_string(required(row, 3, "item_article.site_name")?)?,
        byline: optional_string(required(row, 4, "item_article.byline")?)?,
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
            #[cfg(test)]
            exists: false,
            state: "unread".to_string(),
            progress: None,
            last_scroll_y: None,
        });
    };

    Ok(ReadStateRecord {
        #[cfg(test)]
        exists: true,
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

fn shape_tags(store: &carrel_store::Store, item_id: &str) -> Result<Vec<String>> {
    let Some(record) = store.get_item_shape(item_id)? else {
        return Ok(Vec::new());
    };

    Ok(shape_tag_labels(&record.shape)
        .into_iter()
        .map(str::to_string)
        .collect())
}

fn shape_tag_labels(shape: &Shape) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if shape.has_video_embed {
        tags.push("video");
    }
    if shape.has_audio_embed {
        tags.push("audio");
    }
    if shape.is_link_roundup {
        tags.push("link roundup");
    }
    if shape.is_long_form {
        tags.push("long form");
    }
    if shape.is_short {
        tags.push("short");
    }
    if shape.has_code {
        tags.push("code");
    }
    if shape.has_math {
        tags.push("math");
    }
    tags
}

#[cfg(test)]
fn update_read_progress_in_store(
    store: &carrel_store::Store,
    update: ReadProgressUpdate,
) -> Result<()> {
    store.record_read_progress(
        &update.item_id,
        update.progress,
        update.scroll_y,
        unix_micros(OffsetDateTime::now_utc()),
        &Keypair::from_seed(&[21; 32]),
    )?;
    Ok(())
}

fn write_read_intent(
    store: &carrel_store::Store,
    item_id: &str,
    intent: ReadIntent,
    signer: &Keypair,
) -> Result<()> {
    store.write_private_state(
        StateKey {
            kind: StateKind::Read,
            target: item_id.into(),
        },
        StateValue::Read(intent),
        unix_micros(OffsetDateTime::now_utc()),
        signer,
    )?;
    Ok(())
}

#[cfg(test)]
fn write_read_state(
    store: &carrel_store::Store,
    item_id: &str,
    state: &str,
    progress: Option<f64>,
    scroll_y: Option<f64>,
) -> Result<()> {
    let scroll_y = scroll_y.or(read_state(store, item_id)?.last_scroll_y);
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

#[cfg(test)]
fn toggle_star_in_store(store: &carrel_store::Store, item_id: &str) -> Result<bool> {
    Ok(store.toggle_private_star(
        item_id,
        unix_micros(OffsetDateTime::now_utc()),
        &Keypair::from_seed(&[21; 32]),
    )?)
}

fn readable_html(blobs: &BlobStore, blob_id: &str) -> Result<String> {
    let blob_id = BlobId::from_str(blob_id)?;
    let bytes = blobs.get_blocking(&blob_id)?;
    let html = String::from_utf8(bytes.to_vec()).map_err(|source| AppError::InvalidData {
        context: "item_content.blob",
        value: source.to_string(),
    })?;
    // A content hash proves integrity, and a peer signature proves authorship.
    // Neither makes HTML safe to render inside the application's webview.
    Ok(carrel_feeds::sanitize_cached_html(&html))
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
    starred: bool,
    now: OffsetDateTime,
) -> ItemSummary {
    let (title, summary) = item_display_text(
        &item.title,
        item.primary_url.as_deref(),
        item.summary.as_deref(),
    );
    ItemSummary {
        id: item.id.clone(),
        title,
        source_name: source_name(item.primary_url.as_deref(), article.site_name.as_deref()),
        length_label: length_label(article),
        time_label: time_label(
            item.published_at_micros
                .unwrap_or(item.discovered_at_micros),
            now,
        ),
        read_state,
        starred,
        primary_url: item.primary_url.clone(),
        summary,
        discovered_at_micros: item.discovered_at_micros,
    }
}

fn plain_text_summary(summary: &str) -> String {
    carrel_feeds::html_to_text(summary)
}

fn item_display_text(
    title: &str,
    primary_url: Option<&str>,
    summary_html: Option<&str>,
) -> (String, Option<String>) {
    let summary = summary_html
        .map(plain_text_summary)
        .filter(|summary| !summary.is_empty());
    let title = title.trim();
    if !title.is_empty() && primary_url != Some(title) {
        return (title.to_string(), summary);
    }
    let Some(summary) = summary else {
        return ("Untitled post".to_string(), None);
    };
    let Some((limit, _)) = summary.char_indices().nth(120) else {
        return (summary, None);
    };
    let end = summary[..limit]
        .rfind(' ')
        .filter(|end| *end > 0)
        .unwrap_or(limit);
    let heading = format!("{}…", summary[..end].trim_end());
    let continuation = summary[end..].trim().to_string();
    (heading, (!continuation.is_empty()).then_some(continuation))
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

pub(crate) fn source_name(primary_url: Option<&str>, site_name: Option<&str>) -> String {
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

fn time_label(timestamp_micros: i64, now: OffsetDateTime) -> String {
    let timestamp =
        OffsetDateTime::from_unix_timestamp(timestamp_micros / 1_000_000).unwrap_or(now);
    let age = now - timestamp;

    if age <= Duration::ZERO {
        "now".to_string()
    } else if age < Duration::hours(1) {
        format!("{}m ago", age.whole_minutes().max(1))
    } else if age < Duration::days(1) {
        format!("{}h ago", age.whole_hours())
    } else if age < Duration::days(7) {
        format!("{}d ago", age.whole_days())
    } else {
        let date = timestamp.date();
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        )
    }
}

fn decode_item_row(row: &[DataValue]) -> Result<ItemRecord> {
    decode_item_row_from_offset(row, 0)
}

fn decode_item_row_from_offset(row: &[DataValue], offset: usize) -> Result<ItemRecord> {
    Ok(ItemRecord {
        id: value_as_string(required(row, offset, "item.id")?)?,
        title: value_as_string(required(row, offset + 1, "item.title")?)?,
        creators: value_as_string_list(required(row, offset + 2, "item.creators")?)?,
        primary_url: optional_string(required(row, offset + 3, "item.primary_url")?)?,
        published_at_micros: optional_validity_micros(required(
            row,
            offset + 4,
            "item.published_at",
        )?)?,
        language: optional_string(required(row, offset + 5, "item.language")?)?,
        summary: optional_string(required(row, offset + 6, "item.summary")?)?,
        discovered_at_micros: value_as_validity_micros(required(
            row,
            offset + 7,
            "item.discovered_at",
        )?)?,
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
    use carrel_core::library::ItemView;
    use cozo::Validity;

    use super::*;

    #[test]
    fn untitled_entries_use_plain_opening_text_and_keep_publisher_titles() {
        let url = "https://example.com/untitled";
        assert_eq!(
            item_display_text(
                url,
                Some(url),
                Some("<p>A short opening <em>sentence</em>.</p>")
            ),
            ("A short opening sentence.".into(), None)
        );
        assert_eq!(
            item_display_text("A publisher title", Some(url), Some("<p>The body.</p>")),
            ("A publisher title".into(), Some("The body.".into()))
        );
        assert_eq!(
            item_display_text("  ", Some(url), Some("<script>untrusted()</script>")),
            ("Untitled post".into(), None)
        );
        assert_eq!(
            item_display_text(url, Some(url), None),
            ("Untitled post".into(), None)
        );
        let unicode = "文".repeat(121);
        let (heading, continuation) = item_display_text("", None, Some(&unicode));
        assert_eq!(heading, format!("{}…", "文".repeat(120)));
        assert_eq!(continuation.as_deref(), Some("文"));
        let words = "opening ".repeat(30);
        let (heading, continuation) = item_display_text(url, Some(url), Some(&words));
        assert!(heading.chars().count() <= 121);
        assert!(!heading.ends_with("open…"));
        let reconstructed = format!(
            "{} {}",
            heading.trim_end_matches('…'),
            continuation.unwrap()
        );
        assert_eq!(reconstructed, words.trim());
    }

    #[test]
    fn saved_url_headings_are_presented_consistently_without_rewriting_metadata() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let url = "https://example.com/untitled";
        insert_item(
            &store,
            InsertItem {
                id: "untitled",
                title: url,
                url: Some(url),
                summary: Some("<p>This post has no publisher title.</p>"),
                discovered_at_micros: unix_micros(OffsetDateTime::now_utc()),
                read_state: None,
                site_name: Some("Example"),
                minutes: None,
            },
        );
        let items = list_items_from_store(&store, ItemFilter::default()).unwrap();
        assert_eq!(items[0].title, "This post has no publisher title.");
        assert!(items[0].summary.is_none());
        let tempdir = tempfile::tempdir().unwrap();
        let detail = get_item_from_parts(&store, &BlobStore::open(tempdir.path()), "untitled")
            .unwrap()
            .unwrap();
        assert_eq!(detail.title, items[0].title);
        assert!(
            detail
                .content_html
                .contains("This post has no publisher title.")
        );
        assert_eq!(
            query_item_record(&store, "untitled")
                .unwrap()
                .unwrap()
                .title,
            url
        );
    }

    fn list_items_from_store(
        store: &carrel_store::Store,
        filter: ItemFilter,
    ) -> Result<Vec<ItemSummary>> {
        Ok(list_item_page_from_store(store, filter)?.items)
    }

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
    fn list_and_reader_dates_use_publication_with_arrival_fallback() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        for (id, published_at, expected) in [
            ("dated", Some(1_600_000_000_000_000), "2020-09-13"),
            ("undated", None, "2021-01-01"),
        ] {
            insert_item(
                &store,
                InsertItem {
                    id,
                    title: "An archived post",
                    url: Some("https://example.com/post"),
                    summary: None,
                    discovered_at_micros: 1_609_459_200_000_000,
                    read_state: None,
                    site_name: None,
                    minutes: None,
                },
            );
            store.query_with_params(r#"
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    *item{id: $id, kind, title, creators, primary_url, language, summary, discovered_at},
                    id = $id, published_at = $published_at
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
            "#, BTreeMap::from([
                ("id".into(), DataValue::from(id)),
                ("published_at".into(), published_at.map(|time| DataValue::Validity(Validity::from((time, true)))).unwrap_or(DataValue::Null)),
            ])).unwrap();
            let page = list_item_page_from_store(
                &store,
                ItemFilter {
                    view: ItemView::Library,
                    ..ItemFilter::default()
                },
            )
            .unwrap();
            let row = page.items.iter().find(|item| item.id == id).unwrap();
            assert_eq!(row.time_label, expected);
            assert_eq!(row.discovered_at_micros, 1_609_459_200_000_000);
            let detail = get_item_from_parts(&store, &blobs, id).unwrap().unwrap();
            assert_eq!(detail.time_label, expected);
            assert_eq!(detail.published_at.is_some(), published_at.is_some());
            assert_eq!(detail.discovered_at, "2021-01-01T00:00:00Z");
        }
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
            false,
            OffsetDateTime::now_utc(),
        );

        assert_eq!(summary.summary.as_deref(), Some("Hello world today."));
    }

    #[test]
    fn local_library_filters_cover_read_star_and_feed_surfaces() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let now = OffsetDateTime::now_utc();
        let feed_a = "https://example.com/feed.xml";
        let feed_b = "https://other.example/feed.xml";

        for (id, hours_ago) in [
            ("recent-unread", 1),
            ("older-read", 2),
            ("reading-newer", 6),
            ("reading-older", 3),
            ("starred-newer", 8),
            ("starred-older", 4),
            ("other-feed", 5),
        ] {
            insert_item(
                &store,
                InsertItem {
                    id,
                    title: id,
                    url: Some("https://example.com/item"),
                    summary: None,
                    discovered_at_micros: unix_micros(now - Duration::hours(hours_ago)),
                    read_state: None,
                    site_name: Some("Example"),
                    minutes: Some(3),
                },
            );
        }

        set_item_feed(&store, "recent-unread", feed_a);
        set_item_feed(&store, "older-read", feed_a);
        set_item_feed(&store, "reading-newer", feed_a);
        set_item_feed(&store, "reading-older", feed_a);
        set_item_feed(&store, "starred-newer", feed_a);
        set_item_feed(&store, "starred-older", feed_a);
        set_item_feed(&store, "other-feed", feed_b);
        set_read_state_at(
            &store,
            "older-read",
            "read",
            unix_micros(now - Duration::minutes(60)),
        );
        set_read_state_at(
            &store,
            "reading-newer",
            "reading",
            unix_micros(now - Duration::minutes(5)),
        );
        set_read_state_at(
            &store,
            "reading-older",
            "reading",
            unix_micros(now - Duration::minutes(30)),
        );
        set_star_state_at(
            &store,
            "starred-newer",
            true,
            unix_micros(now - Duration::minutes(3)),
        );
        set_star_state_at(
            &store,
            "starred-older",
            true,
            unix_micros(now - Duration::minutes(45)),
        );

        let library = list_items_from_store(
            &store,
            ItemFilter {
                view: ItemView::Library,
                limit: Some(3),
                after: None,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(
            item_ids(&library),
            vec!["recent-unread", "older-read", "reading-older"]
        );

        let unread = list_items_from_store(
            &store,
            ItemFilter {
                view: ItemView::Unread,
                limit: Some(10),
                after: None,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(
            item_ids(&unread),
            vec![
                "recent-unread",
                "starred-older",
                "other-feed",
                "starred-newer"
            ]
        );

        let reading = list_items_from_store(
            &store,
            ItemFilter {
                view: ItemView::Reading,
                limit: Some(10),
                after: None,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(item_ids(&reading), vec!["reading-newer", "reading-older"]);

        let starred = list_items_from_store(
            &store,
            ItemFilter {
                view: ItemView::Starred,
                limit: Some(10),
                after: None,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(item_ids(&starred), vec!["starred-newer", "starred-older"]);
        assert!(starred.iter().all(|row| row.starred));

        let feed = list_items_from_store(
            &store,
            ItemFilter {
                view: ItemView::Feed {
                    url: feed_a.to_string(),
                },
                limit: Some(10),
                after: None,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(
            item_ids(&feed),
            vec![
                "recent-unread",
                "older-read",
                "reading-older",
                "starred-older",
                "reading-newer",
                "starred-newer"
            ]
        );
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
        store
            .put_item_shape(
                "item-with-content",
                &Shape {
                    is_long_form: true,
                    has_code: true,
                    ..Shape::default()
                },
            )
            .unwrap();
        insert_item_content(&store, "item-with-content", blob_id.as_str(), body.len());

        let detail = get_item_from_parts(&store, &blobs, "item-with-content")
            .unwrap()
            .unwrap();

        assert_eq!(detail.title, "Cached Essay");
        assert_eq!(detail.creators, vec!["Ada"]);
        assert_eq!(detail.source_name, "essays.example");
        assert_eq!(detail.word_count, Some(1200));
        assert_eq!(detail.readable_blob_id.as_deref(), Some(blob_id.as_str()));
        assert_eq!(detail.shape_tags, vec!["long form", "code"]);
        assert_eq!(detail.content_html, "<p>Cached body.</p>");
    }

    #[test]
    fn cached_html_is_sanitized_at_the_rendering_boundary_without_rewriting_its_blob() {
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let raw = br#"<p onclick="steal()">Readable text</p><script>steal()</script>
            <a href="javascript:steal()">Unsafe link</a>
            <img src="blob://cached-image" onerror="steal()">
            <math><mi>x</mi><mo>+</mo><mn>1</mn></math>"#;
        let id = blobs.put_blocking(raw).unwrap();
        let rendered = readable_html(&blobs, &id.to_string()).unwrap();
        assert!(rendered.contains("Readable text"));
        assert!(rendered.contains("blob://cached-image"));
        assert!(rendered.contains("<math>"));
        assert!(!rendered.contains("steal"));
        assert!(!rendered.contains("<script"));
        assert!(!rendered.contains("onclick"));
        assert!(!rendered.contains("onerror"));
        assert!(!rendered.contains("javascript:"));
        assert_eq!(blobs.get_blocking(&id).unwrap().as_ref(), raw);
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

    #[test]
    fn late_progress_preserves_every_explicit_read_state() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        for state in ["read", "unread", "reading", "abandoned", "skimmed"] {
            insert_state_item(&store, state);
            write_read_state(&store, state, state, None, Some(640.0)).unwrap();
            update_read_progress_in_store(
                &store,
                ReadProgressUpdate {
                    item_id: state.into(),
                    progress: 0.25,
                    scroll_y: 800.0,
                },
            )
            .unwrap();
            let saved = read_state(&store, state).unwrap();
            assert_eq!(saved.state, state);
            assert_eq!(saved.last_scroll_y, Some(800.0));
            assert_eq!(
                saved.progress,
                Some(if state == "read" { 1.0 } else { 0.25 })
            );
        }
    }

    #[test]
    fn marking_read_or_unread_keeps_the_saved_position() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        insert_state_item(&store, "one");
        update_read_progress_in_store(
            &store,
            ReadProgressUpdate {
                item_id: "one".into(),
                progress: 0.5,
                scroll_y: 640.0,
            },
        )
        .unwrap();
        write_read_intent(
            &store,
            "one",
            ReadIntent::Read,
            &Keypair::from_seed(&[21; 32]),
        )
        .unwrap();
        assert_eq!(
            read_state(&store, "one").unwrap().last_scroll_y,
            Some(640.0)
        );
        write_read_intent(
            &store,
            "one",
            ReadIntent::Unread,
            &Keypair::from_seed(&[21; 32]),
        )
        .unwrap();
        assert_eq!(
            read_state(&store, "one").unwrap().last_scroll_y,
            Some(640.0)
        );
    }

    #[test]
    fn progress_clamps_bounds_and_rejects_nonfinite_values_without_writing() {
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        insert_state_item(&store, "bounded");
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for (progress, scroll_y) in [(value, 0.0), (0.0, value)] {
                assert!(
                    update_read_progress_in_store(
                        &store,
                        ReadProgressUpdate {
                            item_id: "invalid".into(),
                            progress,
                            scroll_y
                        }
                    )
                    .is_err()
                );
                assert!(!read_state(&store, "invalid").unwrap().exists);
            }
        }
        update_read_progress_in_store(
            &store,
            ReadProgressUpdate {
                item_id: "bounded".into(),
                progress: 1.5,
                scroll_y: -4.0,
            },
        )
        .unwrap();
        let saved = read_state(&store, "bounded").unwrap();
        assert_eq!(saved.progress, Some(1.0));
        assert_eq!(saved.last_scroll_y, Some(0.0));
        update_read_progress_in_store(
            &store,
            ReadProgressUpdate {
                item_id: "bounded".into(),
                progress: -0.5,
                scroll_y: 4.0,
            },
        )
        .unwrap();
        assert_eq!(read_state(&store, "bounded").unwrap().progress, Some(0.0));
    }

    fn insert_state_item(store: &carrel_store::Store, id: &str) {
        insert_item(
            store,
            InsertItem {
                id,
                title: "Article",
                url: None,
                summary: None,
                discovered_at_micros: 1,
                read_state: None,
                site_name: None,
                minutes: None,
            },
        );
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

    fn set_item_feed(store: &carrel_store::Store, item_id: &str, feed_url: &str) {
        store
            .query_with_params(
                r#"
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = $item_id,
                    feed_url = $feed_url,
                    word_count = 1200,
                    estimated_read_minutes = 3,
                    site_name = 'Example',
                    byline = 'Ada'
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
                "#,
                BTreeMap::from([
                    ("item_id".to_string(), DataValue::from(item_id)),
                    ("feed_url".to_string(), DataValue::from(feed_url)),
                ]),
            )
            .unwrap();
    }

    fn set_read_state_at(
        store: &carrel_store::Store,
        item_id: &str,
        state: &str,
        updated_at_micros: i64,
    ) {
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
                    ("item_id".to_string(), DataValue::from(item_id)),
                    ("state".to_string(), DataValue::from(state)),
                    (
                        "updated_at".to_string(),
                        DataValue::Validity(Validity::from((updated_at_micros, true))),
                    ),
                ]),
            )
            .unwrap();
    }

    fn set_star_state_at(
        store: &carrel_store::Store,
        item_id: &str,
        starred: bool,
        updated_at_micros: i64,
    ) {
        store
            .query_with_params(
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
                        DataValue::Validity(Validity::from((updated_at_micros, true))),
                    ),
                ]),
            )
            .unwrap();
    }

    fn item_ids(rows: &[ItemSummary]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
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
