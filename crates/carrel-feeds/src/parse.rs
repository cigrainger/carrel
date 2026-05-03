//! Feed parsing and normalization.

use std::io::Cursor;

use carrel_core::feed::{ParsedEntry, ParsedFeed};
use feed_rs::model::{Entry, Feed, Link, Text};
use html_escape::decode_html_entities;
use url::Url;

use crate::error::ParseError;

/// Parse RSS, Atom, or JSON Feed bytes into Carrel's normalized feed shape.
pub fn parse_feed(bytes: &[u8], feed_url: &str) -> Result<ParsedFeed, ParseError> {
    let base = Url::parse(feed_url).map_err(|source| ParseError::InvalidFeedUrl {
        url: feed_url.to_string(),
        source,
    })?;
    let feed = feed_rs::parser::parse(Cursor::new(bytes))?;

    Ok(ParsedFeed {
        title: feed.title.as_ref().map(text_content),
        description: feed.description.as_ref().map(text_content),
        language: feed.language.clone(),
        entries: feed
            .entries
            .iter()
            .map(|entry| normalize_entry(&feed, entry, &base))
            .collect(),
    })
}

fn normalize_entry(feed: &Feed, entry: &Entry, feed_base: &Url) -> ParsedEntry {
    let entry_base = entry
        .base
        .as_deref()
        .and_then(|raw| Url::parse(raw).or_else(|_| feed_base.join(raw)).ok())
        .unwrap_or_else(|| feed_base.clone());
    let url = best_link(&entry.links).and_then(|link| resolve_url(&link.href, &entry_base));
    let authors = if entry.authors.is_empty() {
        feed.authors
            .iter()
            .map(|author| author.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect()
    } else {
        entry
            .authors
            .iter()
            .map(|author| author.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect()
    };
    let published_at_micros = entry
        .published
        .or(entry.updated)
        .map(|published| published.timestamp_micros());

    ParsedEntry {
        feed_guid: stable_guid(entry, url.as_deref()),
        title: entry.title.as_ref().map(text_content),
        authors,
        url,
        published_at_micros,
        language: entry.language.clone().or_else(|| feed.language.clone()),
        summary_html: entry.summary.as_ref().map(text_content),
        content_html: entry
            .content
            .as_ref()
            .and_then(|content| content.body.as_ref())
            .map(|body| decode_html_entities(body.trim()).to_string()),
    }
}

fn stable_guid(entry: &Entry, url: Option<&str>) -> String {
    let id = entry.id.trim();
    if !id.is_empty() {
        return id.to_string();
    }

    url.map(ToString::to_string)
        .unwrap_or_else(|| "missing-feed-guid".to_string())
}

fn best_link(links: &[Link]) -> Option<&Link> {
    links
        .iter()
        .find(|link| link.rel.as_deref() == Some("alternate"))
        .or_else(|| links.iter().find(|link| link.rel.is_none()))
        .or_else(|| links.first())
}

fn resolve_url(raw: &str, base: &Url) -> Option<String> {
    Url::parse(raw)
        .or_else(|_| base.join(raw))
        .ok()
        .map(|url| url.to_string())
}

fn text_content(text: &Text) -> String {
    decode_html_entities(text.content.trim()).to_string()
}
