//! Fetch article pages and retain publisher metadata and redirect aliases.

use kuchiki::traits::TendrilSink;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

use crate::{ExtractError, ExtractedArticle, FetchResult, Fetcher, extract_from_html};

/// Page extraction and the addresses needed for asset resolution and deduplication.
#[derive(Clone, Debug)]
pub struct ArticlePage {
    /// Address after redirects.
    pub final_url: String,
    /// Publisher's canonical address, falling back to the final URL.
    pub canonical_url: String,
    /// Base address for relative links and assets.
    pub base_url: String,
    /// Sanitized readable article.
    pub article: ExtractedArticle,
    /// Publication time from explicit publisher metadata.
    pub published_at_micros: Option<i64>,
    /// Publisher's description, or an escaped opening excerpt when absent.
    pub summary: Option<String>,
}

/// Fetch a page through the existing robots-aware, rate-limited HTTP boundary.
pub async fn fetch_article(fetcher: &Fetcher, url: &str) -> Result<ArticlePage, ExtractError> {
    let (final_url, result) = fetcher.fetch_document(url, None, None).await?;
    match result {
        FetchResult::Updated { body, .. } => {
            article_from_page(&String::from_utf8_lossy(&body), &final_url)
        }
        FetchResult::NotModified { .. } => Err(ExtractError::NotModified { url: final_url }),
        FetchResult::GoneOrError { status, .. } => Err(ExtractError::HttpStatus {
            url: final_url,
            status,
        }),
    }
}

/// Extract a page without I/O; supplied URLs must describe the fetched page.
pub fn article_from_page(html: &str, final_url: &str) -> Result<ArticlePage, ExtractError> {
    let page_url = Url::parse(final_url).map_err(|source| ExtractError::InvalidUrl {
        url: final_url.into(),
        source,
    })?;
    let document = kuchiki::parse_html().one(html);
    let resolve = |raw: &str, base: &Url| {
        (!raw.trim().is_empty())
            .then(|| base.join(raw.trim()).ok())
            .flatten()
            .filter(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none()
            })
    };
    let base = document
        .select_first("head base[href]")
        .ok()
        .and_then(|node| node.attributes.borrow().get("href").map(str::to_string))
        .and_then(|raw| resolve(&raw, &page_url))
        .unwrap_or_else(|| page_url.clone());
    let canonical = document
        .select("head link[rel][href]")
        .ok()
        .and_then(|mut nodes| {
            nodes.find_map(|node| {
                let attrs = node.attributes.borrow();
                attrs
                    .get("rel")?
                    .split_ascii_whitespace()
                    .any(|rel| rel.eq_ignore_ascii_case("canonical"))
                    .then(|| resolve(attrs.get("href")?, &base))
                    .flatten()
            })
        })
        .unwrap_or_else(|| page_url.clone());
    let mut canonical = canonical;
    canonical.set_fragment(None);
    let meta = |keys: &[&str]| -> Option<String> {
        let nodes: Vec<_> = document.select("head meta[content]").ok()?.collect();
        keys.iter().find_map(|key| {
            nodes.iter().find_map(|node| {
                let attrs = node.attributes.borrow();
                ["property", "name", "itemprop"]
                    .iter()
                    .any(|attr| {
                        attrs
                            .get(*attr)
                            .is_some_and(|name| name.eq_ignore_ascii_case(key))
                    })
                    .then(|| {
                        attrs
                            .get("content")
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
                    .flatten()
            })
        })
    };
    let published = meta(&[
        "article:published_time",
        "datePublished",
        "pubdate",
        "DC.date.issued",
    ])
    .or_else(|| {
        document
            .select_first("time[itemprop='datePublished'][datetime]")
            .ok()
            .and_then(|node| node.attributes.borrow().get("datetime").map(str::to_string))
    });
    let published_at_micros = published
        .and_then(|raw| OffsetDateTime::parse(raw.trim(), &Rfc3339).ok())
        .and_then(|date| i64::try_from(date.unix_timestamp_nanos() / 1000).ok());
    let mut article = extract_from_html(html, base.as_str())?;
    article.title = article
        .title
        .or_else(|| meta(&["og:title", "twitter:title"]));
    article.byline = article.byline.or_else(|| meta(&["author"]));
    article.site_name = meta(&["og:site_name"]).or(article.site_name);
    article.language = article.language.or_else(|| {
        document.select_first("html[lang]").ok().and_then(|node| {
            node.attributes
                .borrow()
                .get("lang")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    });
    let summary = meta(&["description", "og:description", "twitter:description"]).or_else(|| {
        let text = crate::html_to_text(&article.content_html);
        let excerpt: String = text.chars().take(512).collect();
        (!excerpt.is_empty()).then(|| {
            excerpt
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        })
    });
    Ok(ArticlePage {
        final_url: page_url.to_string(),
        canonical_url: canonical.to_string(),
        base_url: base.to_string(),
        article,
        published_at_micros,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(head: &str) -> String {
        format!(
            "<html lang='en'><head><title>A useful article</title>{head}</head><body><article><h1>A useful article</h1><p>{}</p><a href='related'>Related</a></article></body></html>",
            "This article discusses careful reading and the open web in useful detail. ".repeat(25)
        )
    }

    #[test]
    fn metadata_resolves_against_final_address_and_html_base() {
        let saved = article_from_page(&page("<base href='/assets/'><link rel='alternate CANONICAL' href='../article#section'><meta property='article:published_time' content='2026-09-09T08:48:00+10:00'><meta property='og:site_name' content='A publisher'><meta name='description' content='A short description'>"), "https://example.org/moved/index").unwrap();
        assert_eq!(saved.canonical_url, "https://example.org/article");
        assert_eq!(saved.base_url, "https://example.org/assets/");
        assert_eq!(saved.article.site_name.as_deref(), Some("A publisher"));
        assert_eq!(saved.article.language.as_deref(), Some("en"));
        assert_eq!(saved.summary.as_deref(), Some("A short description"));
        assert_eq!(
            saved.published_at_micros,
            Some(
                OffsetDateTime::parse("2026-09-08T22:48:00Z", &Rfc3339)
                    .unwrap()
                    .unix_timestamp()
                    * 1_000_000
            )
        );
        assert!(
            saved
                .article
                .content_html
                .contains("https://example.org/assets/related")
        );
    }

    #[test]
    fn invalid_canonical_and_modified_dates_do_not_supply_identity_or_publication() {
        for link in [
            "",
            "javascript:alert(1)",
            "https://user:password@example.org/page",
        ] {
            let saved = article_from_page(&page(&format!("<link rel='canonical' href='{link}'><meta property='article:modified_time' content='2026-10-11T10:00:00Z'>")), "https://example.org/article").unwrap();
            assert_eq!(saved.canonical_url, "https://example.org/article");
            assert_eq!(saved.published_at_micros, None);
        }
        let saved = article_from_page(
            &page("<meta property='article:published_time' content='invalid'>"),
            "https://example.org/article",
        )
        .unwrap();
        assert_eq!(saved.published_at_micros, None);
    }
}
