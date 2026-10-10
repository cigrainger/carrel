//! Discover validated RSS, Atom, and JSON feeds from a feed or website URL.

use std::collections::BTreeSet;

use kuchiki::traits::TendrilSink;
use url::Url;

use crate::{FetchError, FetchResult, Fetcher, parse_feed};

/// Bound the number of advertised feeds inspected in one discovery operation.
pub const MAX_DISCOVERY_CANDIDATES: usize = 16;
/// Bound document parsing work during URL discovery.
pub const MAX_DISCOVERY_BYTES: usize = 2 * 1024 * 1024;

/// A publisher-advertised feed, validated before it is offered for subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedCandidate {
    /// The feed's resolved HTTP(S) URL.
    pub url: String,
    /// Publisher title, with the advertisement or host as fallback.
    pub title: String,
}

/// A failure while looking for feeds at a supplied URL.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// URL normalization failed.
    #[error("invalid website or feed URL: {0}")]
    InvalidUrl(String),
    /// The polite fetcher rejected or could not fetch the supplied document.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// The supplied URL returned a non-success HTTP status.
    #[error("feed discovery returned HTTP {0}")]
    HttpStatus(u16),
    /// A fresh response body was required.
    #[error("feed discovery received 304 without a document")]
    NotModified,
    /// The supplied document exceeds the parser's work bound.
    #[error("feed discovery document exceeds {MAX_DISCOVERY_BYTES} bytes")]
    TooLarge,
    /// Neither the supplied document nor its advertised links yielded a feed.
    #[error("no valid advertised feed found at {0}")]
    NoFeeds(String),
}

/// Normalize an explicit URL or bare website address without accepting other schemes.
pub fn discovery_url(input: &str) -> Result<Url, DiscoveryError> {
    let input = input.trim();
    let raw = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let mut url = Url::parse(&raw).map_err(|e| DiscoveryError::InvalidUrl(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(DiscoveryError::InvalidUrl(
            "expected an HTTP(S) address".into(),
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

/// Extract standard alternate feed links in document order, respecting the page base.
/// Candidate URLs still need network validation through [`discover_feeds`].
pub fn advertised_feeds(html: &str, page_url: &Url) -> Vec<FeedCandidate> {
    let document = kuchiki::parse_html().one(html);
    let base = document
        .select_first("head base[href]")
        .ok()
        .and_then(|node| node.attributes.borrow().get("href").map(str::to_string))
        .and_then(|href| page_url.join(&href).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or_else(|| page_url.clone());
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let Ok(links) = document.select("link[rel][href]") else {
        return candidates;
    };
    for link in links {
        let attributes = link.attributes.borrow();
        if !attributes
            .get("rel")
            .unwrap_or_default()
            .split_ascii_whitespace()
            .any(|rel| rel.eq_ignore_ascii_case("alternate"))
        {
            continue;
        }
        let mime = attributes
            .get("type")
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !matches!(
            mime.as_str(),
            "application/rss+xml"
                | "application/atom+xml"
                | "application/feed+json"
                | "application/json"
        ) {
            continue;
        }
        let Some(mut url) = attributes
            .get("href")
            .filter(|href| !href.trim().is_empty())
            .and_then(|href| base.join(href.trim()).ok())
        else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        url.set_fragment(None);
        let url = url.to_string();
        if !seen.insert(url.clone()) {
            continue;
        }
        let title = attributes
            .get("title")
            .unwrap_or_default()
            .trim()
            .to_string();
        candidates.push(FeedCandidate { url, title });
        if candidates.len() == MAX_DISCOVERY_CANDIDATES {
            break;
        }
    }
    candidates
}

/// Discover locally through the same robots-aware, rate-limited fetcher as ingestion.
/// Direct feeds are accepted; website advertisements are fetched and parsed before
/// returning candidates. Broken advertisements do not hide other valid feeds.
pub async fn discover_feeds(
    fetcher: &Fetcher,
    input: &str,
) -> Result<Vec<FeedCandidate>, DiscoveryError> {
    let input = discovery_url(input)?;
    let (final_url, response) = fetcher.fetch_document(input.as_str(), None, None).await?;
    let body = match response {
        FetchResult::Updated { body, .. } => body,
        FetchResult::NotModified { .. } => return Err(DiscoveryError::NotModified),
        FetchResult::GoneOrError { status, .. } => return Err(DiscoveryError::HttpStatus(status)),
    };
    if body.len() > MAX_DISCOVERY_BYTES {
        return Err(DiscoveryError::TooLarge);
    }
    let final_url = discovery_url(&final_url)?.to_string();
    if let Ok(feed) = parse_feed(&body, &final_url) {
        return Ok(vec![validated_candidate(final_url, feed.title, "")]);
    }
    let page_url = discovery_url(&final_url)?;
    let advertisements = advertised_feeds(&String::from_utf8_lossy(&body), &page_url);
    let mut seen = BTreeSet::new();
    let mut valid = Vec::new();
    for candidate in advertisements {
        let Ok((url, FetchResult::Updated { body, .. })) =
            fetcher.fetch_document(&candidate.url, None, None).await
        else {
            continue;
        };
        if body.len() > MAX_DISCOVERY_BYTES {
            continue;
        }
        let Ok(url) = discovery_url(&url).map(|url| url.to_string()) else {
            continue;
        };
        let Ok(feed) = parse_feed(&body, &url) else {
            continue;
        };
        if seen.insert(url.clone()) {
            valid.push(validated_candidate(url, feed.title, &candidate.title));
        }
    }
    if valid.is_empty() {
        return Err(DiscoveryError::NoFeeds(final_url));
    }
    Ok(valid)
}

fn validated_candidate(url: String, title: Option<String>, advertisement: &str) -> FeedCandidate {
    let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        if advertisement.is_empty() {
            Url::parse(&url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| url.clone())
        } else {
            advertisement.to_string()
        }
    });
    FeedCandidate { url, title }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_web_addresses_and_rejects_unsupported_or_credentialed_urls() {
        assert_eq!(
            discovery_url(" elixir-lang.org/blog#top ")
                .unwrap()
                .as_str(),
            "https://elixir-lang.org/blog"
        );
        for input in [
            "",
            "file:///tmp/feed.xml",
            "https://user:secret@example.org/feed",
            "javascript:alert(1)",
        ] {
            assert!(discovery_url(input).is_err(), "{input}");
        }
    }

    #[test]
    fn resolves_html_base_and_entities_deduplicates_and_keeps_document_order() {
        let page = Url::parse("https://example.org/blog/index.html").unwrap();
        let links = advertised_feeds(
            r#"<head><base href="/feeds/">
            <link rel="ALTERNATE stylesheet" type="Application/Atom+Xml; charset=utf-8" href="posts.xml#top" title="Posts &amp; essays">
            <link rel="alternate" type="application/atom+xml" href="posts.xml">
            <link rel="alternate" type="application/feed+json" href="comments.json" title="Comments">
            <link rel="alternate" type="text/html" href="print.html">
            <link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">
            <link rel="alternate" type="application/rss+xml" href="https://user:secret@example.org/feed">
            <link rel="alternate" type="application/rss+xml" href="">
            </head>"#,
            &page,
        );
        assert_eq!(
            links,
            vec![
                FeedCandidate {
                    url: "https://example.org/feeds/posts.xml".into(),
                    title: "Posts & essays".into()
                },
                FeedCandidate {
                    url: "https://example.org/feeds/comments.json".into(),
                    title: "Comments".into()
                }
            ]
        );
    }

    #[test]
    fn elixir_blog_relative_atom_link_is_discoverable_without_a_title_attribute() {
        let links = advertised_feeds(
            r#"<link rel="alternate" type="application/atom+xml" href="/atom.xml">"#,
            &Url::parse("https://elixir-lang.org/blog/").unwrap(),
        );
        assert_eq!(links[0].url, "https://elixir-lang.org/atom.xml");
        assert!(links[0].title.is_empty());
    }

    #[test]
    fn validated_titles_use_publisher_then_advertisement_then_host() {
        let url = "https://example.org/feed".to_string();
        assert_eq!(
            validated_candidate(url.clone(), Some("Publisher title".into()), "Advertised").title,
            "Publisher title"
        );
        assert_eq!(
            validated_candidate(url.clone(), Some("  ".into()), "Posts").title,
            "Posts"
        );
        assert_eq!(validated_candidate(url, None, "").title, "example.org");
    }

    #[test]
    fn ignores_unadvertised_anchors_and_bounds_candidate_work() {
        let base = Url::parse("https://example.org/").unwrap();
        assert!(
            advertised_feeds(
                r#"<a rel="alternate" type="application/atom+xml" href="feed.xml">Feed</a>"#,
                &base
            )
            .is_empty()
        );
        let html = (0..100)
            .map(|n| format!(r#"<link rel="alternate" type="application/rss+xml" href="{n}.xml">"#))
            .collect::<String>();
        assert_eq!(
            advertised_feeds(&html, &base).len(),
            MAX_DISCOVERY_CANDIDATES
        );
    }
}
