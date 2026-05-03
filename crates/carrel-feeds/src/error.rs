//! Error types for feed fetching, parsing, and extraction.

use std::io;
use std::time::Duration;

/// Errors raised while fetching a feed or its robots policy.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The requested URL was malformed or unsupported.
    #[error("invalid feed URL {url}: {source}")]
    InvalidUrl {
        /// URL that could not be parsed.
        url: String,
        /// Underlying URL parse error.
        #[source]
        source: url::ParseError,
    },

    /// The HTTP client could not be constructed.
    #[error("failed to build HTTP client: {0}")]
    ClientBuild(#[source] reqwest::Error),

    /// The HTTP request failed before receiving a response.
    #[error("request failed for {url}: {source}")]
    Request {
        /// URL being fetched.
        url: String,
        /// Underlying request error.
        #[source]
        source: reqwest::Error,
    },

    /// The response body could not be read.
    #[error("failed to read response body for {url}: {source}")]
    Body {
        /// URL whose body was being read.
        url: String,
        /// Underlying request error.
        #[source]
        source: reqwest::Error,
    },

    /// robots.txt disallows fetching the target URL.
    #[error("robots.txt disallows {url}")]
    RobotsDisallowed {
        /// URL disallowed by robots.txt.
        url: String,
    },

    /// robots.txt could not be fetched or interpreted safely.
    #[error("robots.txt unavailable for {url}: HTTP {status}")]
    RobotsUnavailable {
        /// URL whose host denied robots policy lookup.
        url: String,
        /// HTTP status returned by robots.txt.
        status: u16,
    },

    /// robots.txt asked us to slow down.
    #[error("robots.txt lookup for {url} was rate-limited")]
    RobotsRateLimited {
        /// URL whose host rate-limited robots policy lookup.
        url: String,
        /// Retry delay, if supplied.
        retry_after: Option<Duration>,
    },

    /// robots.txt was fetched but could not be parsed.
    #[error("failed to parse robots.txt for {url}: {message}")]
    RobotsParse {
        /// URL whose robots policy could not be parsed.
        url: String,
        /// Parser error message.
        message: String,
    },

    /// The rate limiter was closed unexpectedly.
    #[error("rate limiter closed for host {host}")]
    RateLimiterClosed {
        /// Host whose limiter was closed.
        host: String,
    },
}

/// Errors raised while parsing a syndication feed.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The feed URL was malformed.
    #[error("invalid feed URL {url}: {source}")]
    InvalidFeedUrl {
        /// Feed URL passed to the parser.
        url: String,
        /// Underlying URL parse error.
        #[source]
        source: url::ParseError,
    },

    /// feed-rs could not parse the feed bytes.
    #[error("failed to parse feed: {0}")]
    Feed(#[from] feed_rs::parser::ParseFeedError),
}

/// Errors raised while extracting readable article content.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The article URL was malformed or unsupported.
    #[error("invalid article URL {url}: {source}")]
    InvalidUrl {
        /// URL that could not be parsed.
        url: String,
        /// Underlying URL parse error.
        #[source]
        source: url::ParseError,
    },

    /// Fetching the article or an image failed.
    #[error(transparent)]
    Fetch(#[from] FetchError),

    /// The server reported that the article did not change when content was required.
    #[error("article fetch for {url} returned 304 Not Modified without cached content")]
    NotModified {
        /// URL being extracted.
        url: String,
    },

    /// The server returned a non-success status for the article URL.
    #[error("article fetch for {url} returned HTTP {status}")]
    HttpStatus {
        /// URL being extracted.
        url: String,
        /// HTTP status returned by the server.
        status: u16,
    },

    /// The fallback extractor could not be spawned or communicated with.
    #[error("failed to run fallback extractor {command}: {source}")]
    FallbackIo {
        /// Command that failed.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// The fallback extractor exited unsuccessfully.
    #[error("fallback extractor {command} failed with status {status}: {stderr}")]
    FallbackFailed {
        /// Command that failed.
        command: String,
        /// Process exit status.
        status: String,
        /// Standard error captured from the process.
        stderr: String,
    },

    /// The extracted output did not contain readable content.
    #[error("extractor produced no readable content for {url}")]
    EmptyContent {
        /// URL or base URL being extracted.
        url: String,
    },
}
