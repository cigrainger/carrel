//! Joined, filtered archive queries. Filtering precedes page limits.

use std::collections::BTreeMap;

use carrel_core::library::{
    ITEM_PAGE_SIZE, ItemCursor, ItemFilter, ItemPage, ItemView, TODAY_WINDOW_MICROS, search_terms,
};
use cozo::{DataValue, Num};

use crate::{Result, Store, StoreError};

/// Article metadata and personal state fetched together for a list row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryItem {
    /// Latest active vouch, joined with this page when viewing received shares.
    pub shared: Option<LibraryShare>,
    /// Stable item identity.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Original URL.
    pub primary_url: Option<String>,
    /// Feed summary.
    pub summary: Option<String>,
    /// Publisher date, when supplied, used for the visible item date.
    pub published_at_micros: Option<i64>,
    /// Discovery time, independent of this view's sorting time.
    pub discovered_at_micros: i64,
    /// Stored read state, defaulting to unread.
    pub read_state: String,
    /// Stored star state, defaulting to false.
    pub starred: bool,
    /// Source display name, if extracted.
    pub site_name: Option<String>,
    /// Estimated reading minutes, if extracted.
    pub estimated_read_minutes: Option<i64>,
    /// Extracted word count, if known.
    pub word_count: Option<i64>,
}

/// Display context from the verified signed vouch selected for a received row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryShare {
    /// Stable share identity.
    pub id: String,
    /// Locally chosen contact name.
    pub from: String,
    /// Explicitly shared note.
    pub note: Option<String>,
    /// Share creation time, independent of publication and arrival.
    pub created_at_micros: i64,
}

impl Store {
    /// Query one archive page with a single joined store query.
    ///
    /// `since_micros` starts Today's 24-hour publication window. Later pages
    /// retain that window from their cursor. Other views ignore this argument.
    pub fn list_item_page(
        &self,
        filter: &ItemFilter,
        since_micros: i64,
    ) -> Result<ItemPage<LibraryItem>> {
        let terms =
            search_terms(filter.search.as_deref()).map_err(|error| StoreError::InvalidInput {
                context: "item search",
                message: error.to_string(),
            })?;
        if filter.after.as_ref().is_some_and(|cursor| {
            cursor.view != filter.view || cursor.search_terms != terms || cursor.id.is_empty()
        }) {
            return Err(StoreError::InvalidInput {
                context: "item cursor",
                message:
                    "cursor must belong to the requested view and query and contain an item ID"
                        .into(),
            });
        }
        let limit = filter
            .limit
            .unwrap_or(ITEM_PAGE_SIZE)
            .clamp(1, ITEM_PAGE_SIZE);
        let since = if filter.view == ItemView::Today {
            Some(
                filter
                    .after
                    .as_ref()
                    .and_then(|cursor| cursor.since_micros)
                    .unwrap_or(since_micros),
            )
        } else {
            None
        };
        let membership = match &filter.view {
            ItemView::Today => {
                "publication_date[id, sort_at], sort_at >= $since, sort_at <= $until"
            }
            ItemView::Library => "sort_at = to_int(discovered_at)",
            ItemView::Unread => "sort_at = to_int(discovered_at), unread[id]",
            ItemView::Reading => {
                "*read_state{item_id: id, state: 'reading', updated_at}, sort_at = to_int(updated_at)"
            }
            ItemView::Starred => {
                "*item_star{item_id: id, starred: true, updated_at}, sort_at = to_int(updated_at)"
            }
            ItemView::Shared => "shared[id, sort_at]",
            ItemView::Feed { .. } => {
                "*item_article{item_id: id, feed_url: $feed_url}, publication_date[id, sort_at]"
            }
            ItemView::Folder { .. } => {
                "*item_article{item_id: id, feed_url}, *feed{url: feed_url, folder: $folder}, publication_date[id, sort_at]"
            }
        };
        let publication_rules = if matches!(
            filter.view,
            ItemView::Today | ItemView::Feed { .. } | ItemView::Folder { .. }
        ) {
            r#"
            publication_date[id, date] := *item{id, published_at}, published_at != null, date = to_int(published_at)
            publication_date[id, date] := *item{id, published_at: null, discovered_at}, date = to_int(discovered_at)
            "#
        } else {
            ""
        };
        let (search_rules, search_membership) = if terms.is_some() {
            (
                r#"
                terms[term, upper] <- $terms
                hit[id, term] := terms[term, upper], *item:title_words{word, src_id: id}, word >= term, word < upper
                hit[id, term] := terms[term, upper], *item_search:body_words{word, src_item_id: id}, word >= term, word < upper,
                    *item_search{item_id: id, blob_id, extractor_version: $extractor_version},
                    *item_content{item_id: id, format: 'html_readable', blob_id}
                match[id, count_unique(term)] := hit[id, term]
            "#,
                "match[id, $term_count],",
            )
        } else {
            ("", "")
        };
        let shared_context = if filter.view == ItemView::Shared {
            r#"
            chosen[id, min(key)] := page[_, at, id], *social_receipt{share_id, item_id: id}, *social_record{key, share_id, author, recipient, kind: 'share', created_at: at}, not retracted[share_id, author, recipient]
            share_context[id, encoded, name] := chosen[id, key], *social_record{key, encoded, author}, *social_contact{master: author, name}
            "#
        } else {
            "share_context[id, encoded, name] := page[_, _, id], encoded = null, name = null"
        };
        let query = format!(
            r#"
            unread[id] := *read_state{{item_id: id, state: 'unread'}}
            unread[id] := *item{{id}}, not *read_state{{item_id: id}}
            retracted[share_id, author, recipient] := *social_record{{share_id, author, recipient, kind: 'retraction'}}
            shared[id, max(at)] := *social_receipt{{share_id, item_id: id}}, *social_record{{share_id, author, recipient, kind: 'share', created_at: at}}, not retracted[share_id, author, recipient]
            {search_rules}
            {publication_rules}
            eligible[id, sort_at] :=
                *item{{id, discovered_at}},
                not *entity_tombstone{{id}},
                {search_membership}
                {membership},
                (!$has_after || sort_at < $after_time || (sort_at == $after_time && id > $after_id))
            page[rank, sort_at, id] <~ ReorderSort(eligible[id, sort_at],
                out: [sort_at, id], sort_by: [validity(sort_at), id], take: $limit)
            state[id, state] := page[_, _, id], *read_state{{item_id: id, state}}
            state[id, state] := page[_, _, id], not *read_state{{item_id: id}}, state = 'unread'
            star[id, starred] := page[_, _, id], *item_star{{item_id: id, starred}}
            star[id, starred] := page[_, _, id], not *item_star{{item_id: id}}, starred = false
            article[id, site_name, minutes, words] := page[_, _, id], *item_article{{item_id: id, site_name, estimated_read_minutes: minutes, word_count: words}}
            article[id, site_name, minutes, words] := page[_, _, id], not *item_article{{item_id: id}}, site_name = null, minutes = null, words = null
            {shared_context}
            ?[sort_at, id, title, primary_url, summary, discovered_at, state, starred, site_name, minutes, words, published_at, encoded, name] :=
                page[_, sort_at, id],
                *item{{id, title, primary_url, summary, discovered_at, published_at}},
                state[id, state], star[id, starred], article[id, site_name, minutes, words], share_context[id, encoded, name]
            :sort -sort_at, id
        "#
        );
        let rows = self.query_with_params(
            &query,
            BTreeMap::from([
                (
                    "terms".into(),
                    DataValue::List(
                        terms
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .map(|term| {
                                DataValue::List(vec![
                                    DataValue::from(term.as_str()),
                                    DataValue::from(format!("{term}\u{10ffff}")),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "term_count".into(),
                    DataValue::from(terms.as_ref().map_or(0, Vec::len) as i64),
                ),
                (
                    "extractor_version".into(),
                    DataValue::from(crate::search::SEARCH_EXTRACTOR_VERSION),
                ),
                ("since".into(), DataValue::from(since.unwrap_or(0))),
                (
                    "until".into(),
                    DataValue::from(since.unwrap_or(0).saturating_add(TODAY_WINDOW_MICROS)),
                ),
                (
                    "feed_url".into(),
                    DataValue::from(match &filter.view {
                        ItemView::Feed { url } => url.as_str(),
                        _ => "",
                    }),
                ),
                ("has_after".into(), DataValue::Bool(filter.after.is_some())),
                (
                    "folder".into(),
                    DataValue::from(match &filter.view {
                        ItemView::Folder { folder } => folder.as_str(),
                        _ => "",
                    }),
                ),
                (
                    "after_time".into(),
                    DataValue::from(
                        filter
                            .after
                            .as_ref()
                            .map_or(0, |cursor| cursor.sort_at_micros),
                    ),
                ),
                (
                    "after_id".into(),
                    DataValue::from(
                        filter
                            .after
                            .as_ref()
                            .map_or("", |cursor| cursor.id.as_str()),
                    ),
                ),
                (
                    "limit".into(),
                    DataValue::from(i64::try_from(limit + 1).expect("bounded page size")),
                ),
            ]),
        )?;
        let next_cursor = if rows.rows.len() > limit {
            let last = &rows.rows[limit - 1];
            Some(ItemCursor {
                view: filter.view.clone(),
                sort_at_micros: integer(&last[0])?,
                id: string(&last[1])?,
                since_micros: since,
                search_terms: terms,
            })
        } else {
            None
        };
        let items = rows
            .rows
            .iter()
            .take(limit)
            .map(|row| {
                Ok(LibraryItem {
                    shared: match &row[12] {
                        DataValue::Null => None,
                        DataValue::Bytes(bytes) => {
                            match carrel_core::sharing::ShareFact::from_bytes(bytes)? {
                                carrel_core::sharing::ShareFact::Share(share) => {
                                    Some(LibraryShare {
                                        id: share.id,
                                        from: string(&row[13])?,
                                        note: share.note,
                                        created_at_micros: share.created_at_micros,
                                    })
                                }
                                _ => return Err(unexpected("shared article", &row[12])),
                            }
                        }
                        other => return Err(unexpected("shared article", other)),
                    },
                    id: string(&row[1])?,
                    title: string(&row[2])?,
                    primary_url: optional_string(&row[3])?,
                    summary: optional_string(&row[4])?,
                    published_at_micros: match &row[11] {
                        DataValue::Null => None,
                        DataValue::Validity(value) => Some(value.timestamp.0.0),
                        other => return Err(unexpected("published_at", other)),
                    },
                    discovered_at_micros: match &row[5] {
                        DataValue::Validity(value) => value.timestamp.0.0,
                        other => return Err(unexpected("discovered_at", other)),
                    },
                    read_state: string(&row[6])?,
                    starred: match row[7] {
                        DataValue::Bool(value) => value,
                        ref other => return Err(unexpected("starred", other)),
                    },
                    site_name: optional_string(&row[8])?,
                    estimated_read_minutes: match &row[9] {
                        DataValue::Null => None,
                        value => Some(integer(value)?),
                    },
                    word_count: match &row[10] {
                        DataValue::Null => None,
                        value => Some(integer(value)?),
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ItemPage { items, next_cursor })
    }
}

fn string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        other => Err(unexpected("string", other)),
    }
}
fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        value => string(value).map(Some),
    }
}
fn integer(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Num(Num::Int(value)) => Ok(*value),
        other => Err(unexpected("integer", other)),
    }
}
fn unexpected(context: &'static str, value: &DataValue) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: format!("{value:?}"),
    }
}
