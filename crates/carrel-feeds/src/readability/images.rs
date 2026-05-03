//! Image discovery, fetching, and `blob://` URL rewriting.

use std::collections::HashMap;

use bytes::Bytes;
use kuchiki::traits::TendrilSink;
use url::Url;

use crate::{FetchResult, Fetcher};

/// Image URLs that could not be cached while rewriting an article.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageRewriteFailure {
    /// Absolute image URL that failed.
    pub url: String,
    /// Developer-facing explanation.
    pub message: String,
}

/// Result of rewriting an article's image references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageRewriteResult {
    /// HTML with successfully cached images rewritten to `blob://<id>`.
    pub html: String,
    /// Best-effort failures. These do not make article ingest fail.
    pub failures: Vec<ImageRewriteFailure>,
}

/// Fetch and cache article images, rewriting successful references to `blob://` URIs.
pub async fn rewrite_images<E>(
    html: &str,
    base_url: &str,
    fetcher: &Fetcher,
    mut put_blob: impl FnMut(&Bytes) -> Result<String, E>,
) -> ImageRewriteResult
where
    E: std::fmt::Display,
{
    let base = match Url::parse(base_url) {
        Ok(base) => base,
        Err(source) => {
            return ImageRewriteResult {
                html: html.to_string(),
                failures: vec![ImageRewriteFailure {
                    url: base_url.to_string(),
                    message: source.to_string(),
                }],
            };
        }
    };

    let document = kuchiki::parse_html().one(html);
    let mut cache = HashMap::<String, Result<String, String>>::new();
    let mut failures = Vec::new();

    rewrite_src_attributes(
        &document,
        &base,
        fetcher,
        &mut put_blob,
        &mut cache,
        &mut failures,
    )
    .await;
    rewrite_srcset_attributes(
        &document,
        &base,
        fetcher,
        &mut put_blob,
        &mut cache,
        &mut failures,
    )
    .await;

    ImageRewriteResult {
        html: serialize_body_children(&document),
        failures,
    }
}

async fn rewrite_src_attributes<E>(
    document: &kuchiki::NodeRef,
    base: &Url,
    fetcher: &Fetcher,
    put_blob: &mut impl FnMut(&Bytes) -> Result<String, E>,
    cache: &mut HashMap<String, Result<String, String>>,
    failures: &mut Vec<ImageRewriteFailure>,
) where
    E: std::fmt::Display,
{
    let Ok(nodes) = document.select("img[src], source[src], video[poster]") else {
        return;
    };

    for node in nodes {
        let attr_name = if node.attributes.borrow().contains("poster") {
            "poster"
        } else {
            "src"
        };
        let Some(raw) = node
            .attributes
            .borrow()
            .get(attr_name)
            .map(ToString::to_string)
        else {
            continue;
        };
        let Some(url) = absolute_url(&raw, base) else {
            continue;
        };

        if let Some(blob_url) = fetch_image(&url, fetcher, put_blob, cache, failures).await {
            node.attributes.borrow_mut().insert(attr_name, blob_url);
        }
    }
}

async fn rewrite_srcset_attributes<E>(
    document: &kuchiki::NodeRef,
    base: &Url,
    fetcher: &Fetcher,
    put_blob: &mut impl FnMut(&Bytes) -> Result<String, E>,
    cache: &mut HashMap<String, Result<String, String>>,
    failures: &mut Vec<ImageRewriteFailure>,
) where
    E: std::fmt::Display,
{
    let Ok(nodes) = document.select("img[srcset], source[srcset]") else {
        return;
    };

    for node in nodes {
        let Some(raw) = node
            .attributes
            .borrow()
            .get("srcset")
            .map(ToString::to_string)
        else {
            continue;
        };

        let mut rewritten = Vec::new();
        for candidate in parse_srcset(&raw) {
            let Some(url) = absolute_url(&candidate.url, base) else {
                rewritten.push(candidate.original);
                continue;
            };
            if let Some(blob_url) = fetch_image(&url, fetcher, put_blob, cache, failures).await {
                rewritten.push(match candidate.descriptor {
                    Some(descriptor) => format!("{blob_url} {descriptor}"),
                    None => blob_url,
                });
            } else {
                rewritten.push(candidate.original);
            }
        }

        node.attributes
            .borrow_mut()
            .insert("srcset", rewritten.join(", "));
    }
}

async fn fetch_image<E>(
    url: &str,
    fetcher: &Fetcher,
    put_blob: &mut impl FnMut(&Bytes) -> Result<String, E>,
    cache: &mut HashMap<String, Result<String, String>>,
    failures: &mut Vec<ImageRewriteFailure>,
) -> Option<String>
where
    E: std::fmt::Display,
{
    if !cache.contains_key(url) {
        let result = match fetcher.fetch(url, None, None).await {
            Ok(FetchResult::Updated { body, .. }) => put_blob(&body)
                .map(|id| format!("blob://{id}"))
                .map_err(|source| source.to_string()),
            Ok(FetchResult::NotModified { .. }) => Err("HTTP 304 without cached image".to_string()),
            Ok(FetchResult::GoneOrError { status, .. }) => Err(format!("HTTP {status}")),
            Err(source) => Err(source.to_string()),
        };
        cache.insert(url.to_string(), result);
    }

    match cache.get(url).expect("cache populated") {
        Ok(blob_url) => Some(blob_url.clone()),
        Err(message) => {
            if !failures.iter().any(|failure| failure.url == url) {
                failures.push(ImageRewriteFailure {
                    url: url.to_string(),
                    message: message.clone(),
                });
            }
            None
        }
    }
}

fn absolute_url(raw: &str, base: &Url) -> Option<String> {
    if raw.starts_with("blob://") {
        return None;
    }

    Url::parse(raw)
        .or_else(|_| base.join(raw))
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.to_string())
}

#[derive(Debug)]
struct SrcsetCandidate {
    original: String,
    url: String,
    descriptor: Option<String>,
}

fn parse_srcset(raw: &str) -> Vec<SrcsetCandidate> {
    raw.split(',')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(|candidate| {
            let mut parts = candidate.split_whitespace();
            let url = parts.next().unwrap_or_default().to_string();
            let descriptor = parts.collect::<Vec<_>>().join(" ");
            SrcsetCandidate {
                original: candidate.to_string(),
                url,
                descriptor: (!descriptor.is_empty()).then_some(descriptor),
            }
        })
        .collect()
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
