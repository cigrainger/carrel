//! Archived open-web articles, independent of fetching and storage.

use crate::shape::Shape;

/// A readable article whose HTML and images have already been cached.
#[derive(Clone, Debug)]
pub struct ArchivedArticle {
    /// Publisher's canonical HTTP(S) URL, or the final fetched address.
    pub url: String,
    /// Supplied and redirected addresses identifying this same article.
    pub aliases: Vec<String>,
    /// Publisher title, if available.
    pub title: Option<String>,
    /// Extracted author display names.
    pub creators: Vec<String>,
    /// Publisher's publication timestamp, never the fetch time.
    pub published_at_micros: Option<i64>,
    /// Language tag, if available.
    pub language: Option<String>,
    /// Publisher description or opening excerpt, if available.
    pub summary: Option<String>,
    /// Source display name.
    pub site_name: Option<String>,
    /// Extracted byline.
    pub byline: Option<String>,
    /// Content address of sanitized readable HTML.
    pub blob_id: String,
    /// Exact readable HTML length.
    pub byte_size: i64,
    /// Extractor that produced the archive.
    pub extracted_with: String,
    /// Word count of readable content.
    pub word_count: i64,
    /// Estimated reading time in minutes.
    pub estimated_read_minutes: i64,
    /// Derived plain text for local search.
    pub searchable_text: String,
    /// Derived content-shape facts.
    pub shape: Shape,
}
