//! Atomic URL capture into the existing article library.

use std::collections::{BTreeMap, BTreeSet};

use carrel_core::article::ArchivedArticle;
use carrel_core::private_library::{ArticleMetadata, BlobReference, ContentReference, SavedItem};
use cozo::{DataValue, Validity};

use crate::ids::{canonicalize_external_identifier, id_for_external};
use crate::{Result, Store, StoreError};

/// Outcome of saving an archive, retaining existing article identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleSave {
    /// Stable item ID.
    pub id: String,
    /// Stored title, which may already have come from a feed.
    pub title: String,
    /// Whether this item was already in the library.
    pub existed: bool,
    /// Whether this save supplied or recovered the readable cache.
    pub archived: bool,
}

impl Store {
    /// Find a saved article by any known normalized URL, without merging targets.
    pub fn article_by_urls(&self, urls: &[String]) -> Result<Option<String>> {
        let _guard = self.private_state_lock.lock().expect("article write lock");
        self.article_by_urls_unlocked(urls)
    }

    fn article_by_urls_unlocked(&self, urls: &[String]) -> Result<Option<String>> {
        let urls = urls
            .iter()
            .map(|url| article_url(url))
            .collect::<Result<BTreeSet<_>>>()?;
        let rows = self.query_with_params(
            r#"
            input[value] <- $urls
            found[id] := input[value], *item_identifier{item_id: id, scheme: 'url', value}
            found[id] := input[value], *item{id, primary_url: value}
            removed[id] := found[id], *entity_tombstone{id}
            candidate[id, deleted] := found[id], removed[id], deleted = true
            candidate[id, deleted] := found[id], not removed[id], deleted = false
            ?[id, kind, deleted] := candidate[id, deleted], *item{id, kind}
            :sort id
        "#,
            BTreeMap::from([(
                "urls".into(),
                DataValue::List(
                    urls.into_iter()
                        .map(|url| DataValue::List(vec![DataValue::from(url)]))
                        .collect(),
                ),
            )]),
        )?;
        if rows.rows.len() > 1 {
            return Err(invalid(
                "article addresses already identify different saved items",
            ));
        }
        rows.rows
            .first()
            .map(|row| match (&row[0], &row[1], &row[2]) {
                (DataValue::Str(id), DataValue::Str(kind), DataValue::Bool(false))
                    if kind == "article" =>
                {
                    Ok(id.to_string())
                }
                _ => Err(invalid(
                    "article address belongs to a removed or non-article item",
                )),
            })
            .transpose()
    }

    /// Save one archive atomically, preserving any existing cache and personal facts.
    ///
    /// Callers must store the referenced blob before this operation. Source metadata
    /// and the derived search/shape facts commit together. Existing dirty-item
    /// triggers provide private replication through the ordinary capture worker.
    /// `missing_blob` permits replacing only that exact unavailable reference;
    /// a concurrently recovered cache is preserved.
    pub fn save_archived_article(
        &self,
        archive: &ArchivedArticle,
        at_micros: i64,
        missing_blob: Option<&str>,
    ) -> Result<ArticleSave> {
        let _guard = self.private_state_lock.lock().expect("article write lock");
        let url = article_url(&archive.url)?;
        let mut urls = archive
            .aliases
            .iter()
            .map(|url| article_url(url))
            .collect::<Result<BTreeSet<_>>>()?;
        urls.insert(url.clone());
        if urls.len() > 128 {
            return Err(invalid("too many article URL aliases"));
        }
        let existing = self.article_by_urls_unlocked(&urls.iter().cloned().collect::<Vec<_>>())?;
        let id = existing.clone().unwrap_or_else(|| id_for_external(&url));
        let title = archive
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(&url);
        // Reuse the portable metadata bounds before writing any materialization.
        SavedItem {
            id: id.clone(),
            kind: "article".into(),
            title: title.into(),
            creators: archive.creators.clone(),
            primary_url: Some(url.clone()),
            published_at_micros: archive.published_at_micros,
            language: archive.language.clone(),
            summary: archive.summary.clone(),
            discovered_at_micros: at_micros,
            article: Some(ArticleMetadata {
                feed_url: None,
                word_count: Some(archive.word_count),
                estimated_read_minutes: Some(archive.estimated_read_minutes),
                site_name: archive.site_name.clone(),
                byline: archive.byline.clone(),
            }),
            identifiers: Vec::new(),
            resources: Vec::new(),
            contents: vec![ContentReference {
                format: "html_readable".into(),
                blob: BlobReference {
                    blob_id: archive.blob_id.clone(),
                    byte_size: u64::try_from(archive.byte_size)
                        .map_err(|_| invalid("negative article cache size"))?,
                },
                fetched_at_micros: at_micros,
                extracted_with: Some(archive.extracted_with.clone()),
            }],
        }
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
        let current = self.get_item_detail(&id)?;
        let fill_cache = current.as_ref().is_none_or(|item| {
            item.readable
                .as_ref()
                .is_none_or(|content| Some(content.blob_id.as_str()) == missing_blob)
        });
        let primary_url = current
            .as_ref()
            .and_then(|item| item.primary_url.as_deref())
            .unwrap_or(&url);
        let identifier_rows = urls
            .iter()
            .map(|value| {
                DataValue::List(vec![
                    DataValue::from(value.as_str()),
                    DataValue::Bool(value == primary_url),
                ])
            })
            .collect();
        let shape = &archive.shape;
        self.query_with_params(r#"
            {
                ?[id] := *item{id: $id, kind}, id = $id, kind != 'article'
                :assert none
            }
            {
                base[title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    *item{id: $id, title, creators, primary_url, published_at, language, summary, discovered_at}
                base[title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    not *item{id: $id}, title = $title, creators = $creators,
                    primary_url = $url, published_at = $published, language = $language,
                    summary = $summary, discovered_at = $at
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = $id, kind = 'article', base[title, creators, primary_url, published_at, language, summary, discovered_at]
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
            }
            {
                input[value, canonical] <- $identifiers
                arrival[value, discovered_at] := input[value, _], *item_identifier{item_id: $id, scheme: 'url', value, discovered_at}
                arrival[value, discovered_at] := input[value, _], not *item_identifier{item_id: $id, scheme: 'url', value}, discovered_at = $at
                ?[item_id, scheme, value, is_canonical, discovered_at] :=
                    input[value, is_canonical], arrival[value, discovered_at], item_id = $id, scheme = 'url'
                :put item_identifier {item_id, scheme, value => is_canonical, discovered_at}
            }
            {
                source[feed_url, site_name, byline] := *item_article{item_id: $id, feed_url, site_name, byline}
                source[feed_url, site_name, byline] := not *item_article{item_id: $id}, feed_url = null, site_name = $site, byline = $byline
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    $fill, source[feed_url, site, author], item_id = $id,
                    word_count = $words, estimated_read_minutes = $minutes,
                    site_name = if(is_null(site), $site, site), byline = if(is_null(author), $byline, author)
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
            }
            {
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    $fill, item_id = $id, format = 'html_readable', blob_id = $blob,
                    fetched_at = $at, extracted_with = $extractor, byte_size = $size
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
            }
            {
                ?[item_id, blob_id, text, extractor_version] :=
                    $fill, item_id = $id, blob_id = $blob, text = $text, extractor_version = $search_version
                :put item_search {item_id => blob_id, text, extractor_version}
            }
            {
                ?[item_id, has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at] :=
                    $fill, item_id = $id, has_video_embed = $video, has_audio_embed = $audio,
                    is_link_roundup = $roundup, is_long_form = $long, is_short = $short,
                    has_code = $code, has_math = $math, detected_at = $at
                :put item_shape {item_id => has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at}
            }
        "#, BTreeMap::from([
            ("id".into(), DataValue::from(id.as_str())), ("url".into(), DataValue::from(url.as_str())),
            ("title".into(), DataValue::from(title)), ("creators".into(), DataValue::List(archive.creators.iter().map(|s| DataValue::from(s.as_str())).collect())),
            ("published".into(), archive.published_at_micros.map(validity).unwrap_or(DataValue::Null)),
            ("language".into(), optional(&archive.language)), ("summary".into(), optional(&archive.summary)),
            ("at".into(), validity(at_micros)), ("identifiers".into(), DataValue::List(identifier_rows)),
            ("fill".into(), DataValue::Bool(fill_cache)), ("site".into(), optional(&archive.site_name)),
            ("byline".into(), optional(&archive.byline)), ("words".into(), DataValue::from(archive.word_count)),
            ("minutes".into(), DataValue::from(archive.estimated_read_minutes)),
            ("blob".into(), DataValue::from(archive.blob_id.as_str())), ("size".into(), DataValue::from(archive.byte_size)),
            ("extractor".into(), DataValue::from(archive.extracted_with.as_str())),
            ("text".into(), DataValue::from(archive.searchable_text.as_str())),
            ("search_version".into(), DataValue::from(crate::search::SEARCH_EXTRACTOR_VERSION)),
            ("video".into(), DataValue::Bool(shape.has_video_embed)), ("audio".into(), DataValue::Bool(shape.has_audio_embed)),
            ("roundup".into(), DataValue::Bool(shape.is_link_roundup)), ("long".into(), DataValue::Bool(shape.is_long_form)),
            ("short".into(), DataValue::Bool(shape.is_short)), ("code".into(), DataValue::Bool(shape.has_code)),
            ("math".into(), DataValue::Bool(shape.has_math)),
        ]))?;
        Ok(ArticleSave {
            id,
            title: current
                .map(|item| item.title)
                .unwrap_or_else(|| title.into()),
            existed: existing.is_some(),
            archived: fill_cache,
        })
    }
}

fn article_url(raw: &str) -> Result<String> {
    let mut url = url::Url::parse(raw).map_err(|source| StoreError::InvalidUrl {
        url: raw.into(),
        source,
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || raw.len() > 4096
    {
        return Err(invalid("expected an HTTP(S) article address"));
    }
    url.set_fragment(None);
    Ok(canonicalize_external_identifier(url.as_str()))
}

fn optional(value: &Option<String>) -> DataValue {
    value
        .as_deref()
        .map(DataValue::from)
        .unwrap_or(DataValue::Null)
}
fn validity(at: i64) -> DataValue {
    DataValue::Validity(Validity::from((at, true)))
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "article capture",
        message: message.into(),
    }
}
