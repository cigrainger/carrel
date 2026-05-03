//! Readable article extraction, sanitization, and asset rewriting.

mod extract;
mod images;
mod sanitize;

pub use extract::{
    ExtractOptions, ExtractedArticle, ExtractorUsed, TrafilaturaConfig, extract_embedded_html,
    extract_from_html, extract_from_html_with_options, extract_from_url,
    extract_from_url_with_options, html_to_text,
};
pub use images::{ImageRewriteFailure, ImageRewriteResult, rewrite_images};
pub use sanitize::sanitize_html;

pub(crate) const READABILITY_EXTRACTOR: &str = "readable-readability";
pub(crate) const TRAFILATURA_EXTRACTOR: &str = "trafilatura";
pub(crate) const FEED_EMBEDDED_EXTRACTOR: &str = "feed-embedded";
