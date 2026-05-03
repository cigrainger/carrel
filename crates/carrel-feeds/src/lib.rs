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

pub use crate::error::{FetchError, ParseError};
pub use crate::fetch::{DEFAULT_USER_AGENT, FetchResult, Fetcher, FetcherConfig, HttpHeaders};
pub use crate::parse::parse_feed;
