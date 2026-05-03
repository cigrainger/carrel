//! carrel-feeds: feed fetching, parsing, and content extraction.
//!
//! This crate owns open-web ingestion. It depends on core types, but not on the
//! store, sync layer, CLI, or desktop shell.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod fetch;
mod parse;
mod politeness;
pub mod readability;
pub mod shape;

pub use crate::error::{ExtractError, FetchError, ParseError};
pub use crate::fetch::{DEFAULT_USER_AGENT, FetchResult, Fetcher, FetcherConfig, HttpHeaders};
pub use crate::parse::parse_feed;
pub use crate::readability::{
    ExtractOptions, ExtractedArticle, ExtractorUsed, ImageRewriteFailure, ImageRewriteResult,
    TrafilaturaConfig, extract_embedded_html, extract_from_html, extract_from_html_with_options,
    extract_from_url, extract_from_url_with_options, html_to_text, rewrite_images, sanitize_html,
};
pub use crate::shape::detect_shape;
