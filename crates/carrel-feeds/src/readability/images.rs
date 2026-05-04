//! Image discovery, fetching, and `blob://` URL rewriting.

use std::collections::HashMap;

use bytes::Bytes;
use kuchiki::traits::TendrilSink;
use url::Url;

use crate::{FetchResult, Fetcher};

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedImage {
    blob_url: String,
    dimensions: Option<ImageDimensions>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImageDimensions {
    width: u32,
    height: u32,
}

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
    let mut cache = HashMap::<String, Result<CachedImage, String>>::new();
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
    cache: &mut HashMap<String, Result<CachedImage, String>>,
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

        if let Some(image) = fetch_image(&url, fetcher, put_blob, cache, failures).await {
            let mut attributes = node.attributes.borrow_mut();
            attributes.insert(attr_name, image.blob_url);
            if attr_name == "src"
                && node.name.local.as_ref() == "img"
                && let Some(dimensions) = image.dimensions
            {
                attributes.insert("width", dimensions.width.to_string());
                attributes.insert("height", dimensions.height.to_string());
            }
        }
    }
}

async fn rewrite_srcset_attributes<E>(
    document: &kuchiki::NodeRef,
    base: &Url,
    fetcher: &Fetcher,
    put_blob: &mut impl FnMut(&Bytes) -> Result<String, E>,
    cache: &mut HashMap<String, Result<CachedImage, String>>,
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
            if let Some(image) = fetch_image(&url, fetcher, put_blob, cache, failures).await {
                rewritten.push(match candidate.descriptor {
                    Some(descriptor) => format!("{} {descriptor}", image.blob_url),
                    None => image.blob_url,
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
    cache: &mut HashMap<String, Result<CachedImage, String>>,
    failures: &mut Vec<ImageRewriteFailure>,
) -> Option<CachedImage>
where
    E: std::fmt::Display,
{
    if !cache.contains_key(url) {
        let result = match fetcher.fetch(url, None, None).await {
            Ok(FetchResult::Updated { body, .. }) => {
                let dimensions = image_dimensions(&body);
                put_blob(&body)
                    .map(|id| CachedImage {
                        blob_url: format!("blob://{id}"),
                        dimensions,
                    })
                    .map_err(|source| source.to_string())
            }
            Ok(FetchResult::NotModified { .. }) => Err("HTTP 304 without cached image".to_string()),
            Ok(FetchResult::GoneOrError { status, .. }) => Err(format!("HTTP {status}")),
            Err(source) => Err(source.to_string()),
        };
        cache.insert(url.to_string(), result);
    }

    match cache.get(url).expect("cache populated") {
        Ok(image) => Some(image.clone()),
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

fn image_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    png_dimensions(bytes)
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| webp_dimensions(bytes))
}

fn png_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }

    Some(ImageDimensions {
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    })
    .filter(|dimensions| dimensions.width > 0 && dimensions.height > 0)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 4 || !bytes.starts_with(b"\xff\xd8") {
        return None;
    }

    let mut index = 2;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }

        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        let marker = *bytes.get(index)?;
        index += 1;

        if matches!(marker, 0xd8 | 0xd9 | 0x01) {
            continue;
        }
        if index + 2 > bytes.len() {
            return None;
        }

        let len = usize::from(u16::from_be_bytes(bytes[index..index + 2].try_into().ok()?));
        if len < 2 || index + len > bytes.len() {
            return None;
        }

        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) && len >= 7
        {
            let height = u32::from(u16::from_be_bytes(
                bytes[index + 3..index + 5].try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                bytes[index + 5..index + 7].try_into().ok()?,
            ));
            return Some(ImageDimensions { width, height })
                .filter(|dimensions| dimensions.width > 0 && dimensions.height > 0);
        }

        index += len;
    }

    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        return None;
    }

    if &bytes[12..16] == b"VP8X" {
        let width = 1 + read_webp_u24(&bytes[24..27])?;
        let height = 1 + read_webp_u24(&bytes[27..30])?;
        return Some(ImageDimensions { width, height });
    }

    None
}

fn read_webp_u24(bytes: &[u8]) -> Option<u32> {
    Some(
        u32::from(*bytes.first()?)
            | (u32::from(*bytes.get(1)?) << 8)
            | (u32::from(*bytes.get(2)?) << 16),
    )
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
