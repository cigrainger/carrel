//! Article extraction using readable-readability plus an opt-in subprocess fallback.

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use kuchiki::traits::TendrilSink;
use readable_readability::Readability;
use url::Url;

use crate::error::ExtractError;
use crate::{FetchResult, Fetcher};

use super::sanitize::sanitize_html;
use super::{FEED_EMBEDDED_EXTRACTOR, READABILITY_EXTRACTOR, TRAFILATURA_EXTRACTOR};

const WORDS_PER_MINUTE: usize = 250;

/// Which extractor produced the readable article body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtractorUsed {
    /// The entry already carried full content in the feed.
    FeedEmbedded,
    /// Mozilla-style readability extraction via `readable-readability`.
    ReadableReadability,
    /// Opt-in subprocess fallback, usually the Python `trafilatura` CLI.
    Trafilatura,
}

impl ExtractorUsed {
    /// Return the stable label persisted in `item_content.extracted_with`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FeedEmbedded => FEED_EMBEDDED_EXTRACTOR,
            Self::ReadableReadability => READABILITY_EXTRACTOR,
            Self::Trafilatura => TRAFILATURA_EXTRACTOR,
        }
    }
}

/// Optional extraction settings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtractOptions {
    /// Fallback extractor command. Disabled when absent.
    pub trafilatura: Option<TrafilaturaConfig>,
}

/// Configuration for a subprocess fallback extractor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrafilaturaConfig {
    /// Command to execute.
    pub command: OsString,
    /// Arguments to pass before writing HTML to stdin.
    pub args: Vec<OsString>,
}

impl TrafilaturaConfig {
    /// Build a fallback command from a program name and arguments.
    pub fn new(command: impl Into<OsString>, args: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            command: command.into(),
            args: args.into_iter().collect(),
        }
    }
}

/// Sanitized readable article content and metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedArticle {
    /// Extracted article title, if present.
    pub title: Option<String>,
    /// Extracted byline text, if present.
    pub byline: Option<String>,
    /// Cleaned HTML body ready for blob storage.
    pub content_html: String,
    /// Word count computed from the sanitized HTML.
    pub word_count: usize,
    /// Estimated reading time in minutes.
    pub estimated_read_minutes: u32,
    /// Extracted language tag, if known.
    pub language: Option<String>,
    /// Site name inferred from the article URL, if known.
    pub site_name: Option<String>,
    /// Extractor that produced this content.
    pub extractor: ExtractorUsed,
}

/// Extract an article body from full-page HTML.
pub fn extract_from_html(html: &str, base_url: &str) -> Result<ExtractedArticle, ExtractError> {
    extract_from_html_with_options(html, base_url, &ExtractOptions::default())
}

/// Extract an article body from full-page HTML with optional fallback settings.
pub fn extract_from_html_with_options(
    html: &str,
    base_url: &str,
    options: &ExtractOptions,
) -> Result<ExtractedArticle, ExtractError> {
    match readable_extract(html, base_url) {
        Ok(primary) if primary.word_count > 0 || options.trafilatura.is_none() => Ok(primary),
        Ok(_) | Err(ExtractError::EmptyContent { .. }) => run_trafilatura(
            html,
            base_url,
            options
                .trafilatura
                .as_ref()
                .expect("fallback is configured"),
        ),
        Err(error) => Err(error),
    }
}

/// Sanitize already embedded feed content without fetching or readability scoring.
pub fn extract_embedded_html(html: &str, base_url: &str) -> Result<ExtractedArticle, ExtractError> {
    let base = parse_base_url(base_url)?;
    let content_html = sanitize_html(html);
    article_from_sanitized(
        None,
        None,
        content_html,
        None,
        site_name(&base),
        ExtractorUsed::FeedEmbedded,
        base_url,
    )
}

/// Fetch a URL politely and extract readable content.
pub async fn extract_from_url(
    fetcher: &Fetcher,
    url: &str,
) -> Result<ExtractedArticle, ExtractError> {
    extract_from_url_with_options(fetcher, url, &ExtractOptions::default()).await
}

/// Fetch a URL politely and extract readable content with optional fallback settings.
pub async fn extract_from_url_with_options(
    fetcher: &Fetcher,
    url: &str,
    options: &ExtractOptions,
) -> Result<ExtractedArticle, ExtractError> {
    match fetcher.fetch(url, None, None).await? {
        FetchResult::Updated { body, .. } => {
            extract_from_html_with_options(&body_text(&body), url, options)
        }
        FetchResult::NotModified { .. } => Err(ExtractError::NotModified {
            url: url.to_string(),
        }),
        FetchResult::GoneOrError { status, .. } => Err(ExtractError::HttpStatus {
            url: url.to_string(),
            status,
        }),
    }
}

/// Convert sanitized HTML into plain text for previews and tests.
pub fn html_to_text(html: &str) -> String {
    kuchiki::parse_html()
        .one(html)
        .text_contents()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn reading_stats(html: &str) -> (usize, u32) {
    let word_count = html_to_text(html).split_whitespace().count();
    let minutes = if word_count == 0 {
        0
    } else {
        word_count.div_ceil(WORDS_PER_MINUTE).max(1)
    };

    (word_count, minutes.try_into().unwrap_or(u32::MAX))
}

fn readable_extract(html: &str, base_url: &str) -> Result<ExtractedArticle, ExtractError> {
    let base = parse_base_url(base_url)?;
    let (node, metadata) = Readability::new().base_url(base.clone()).parse(html);
    let content_html = sanitize_html(&node.to_string());

    article_from_sanitized(
        metadata.article_title.or(metadata.page_title),
        metadata.byline,
        content_html,
        None,
        site_name(&base),
        ExtractorUsed::ReadableReadability,
        base_url,
    )
}

fn run_trafilatura(
    html: &str,
    base_url: &str,
    config: &TrafilaturaConfig,
) -> Result<ExtractedArticle, ExtractError> {
    let command = config.command.to_string_lossy().to_string();
    let mut child = Command::new(&config.command)
        .args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| ExtractError::FallbackIo {
            command: command.clone(),
            source,
        })?;

    child
        .stdin
        .as_mut()
        .expect("stdin was configured")
        .write_all(html.as_bytes())
        .map_err(|source| ExtractError::FallbackIo {
            command: command.clone(),
            source,
        })?;

    let output = child
        .wait_with_output()
        .map_err(|source| ExtractError::FallbackIo {
            command: command.clone(),
            source,
        })?;

    if !output.status.success() {
        return Err(ExtractError::FallbackFailed {
            command,
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let base = parse_base_url(base_url)?;
    let content_html = sanitize_html(&String::from_utf8_lossy(&output.stdout));
    article_from_sanitized(
        None,
        None,
        content_html,
        None,
        site_name(&base),
        ExtractorUsed::Trafilatura,
        base_url,
    )
}

fn article_from_sanitized(
    title: Option<String>,
    byline: Option<String>,
    content_html: String,
    language: Option<String>,
    site_name: Option<String>,
    extractor: ExtractorUsed,
    base_url: &str,
) -> Result<ExtractedArticle, ExtractError> {
    let (word_count, estimated_read_minutes) = reading_stats(&content_html);
    if content_html.trim().is_empty() {
        return Err(ExtractError::EmptyContent {
            url: base_url.to_string(),
        });
    }

    Ok(ExtractedArticle {
        title: normalize_text(title),
        byline: normalize_text(byline),
        content_html,
        word_count,
        estimated_read_minutes,
        language,
        site_name,
        extractor,
    })
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).to_string()
}

fn parse_base_url(raw: &str) -> Result<Url, ExtractError> {
    Url::parse(raw).map_err(|source| ExtractError::InvalidUrl {
        url: raw.to_string(),
        source,
    })
}

fn site_name(url: &Url) -> Option<String> {
    url.host_str()
        .map(|host| host.strip_prefix("www.").unwrap_or(host).to_string())
}

fn normalize_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}
