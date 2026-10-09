//! Shared library query contracts, independent of storage and presentation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The number of articles requested in one archive page.
pub const ITEM_PAGE_SIZE: usize = 100;

/// Maximum UTF-8 size of a literal search query.
pub const MAX_SEARCH_BYTES: usize = 2_048;
/// Maximum number of distinct words in a search query.
pub const MAX_SEARCH_TERMS: usize = 64;

/// A query exceeds the bounds of interactive archive search.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SearchError {
    /// The raw query is too long.
    #[error("search query exceeds {MAX_SEARCH_BYTES} bytes")]
    TooLong,
    /// The query contains too many distinct words.
    #[error("search query exceeds {MAX_SEARCH_TERMS} distinct words")]
    TooManyTerms,
}

/// Literal words matching Cozo's Simple tokenizer and Lowercase filter.
///
/// An absent or blank query disables search. Punctuation alone is an active
/// search with no words, so it matches nothing rather than the whole archive.
pub fn search_terms(query: Option<&str>) -> Result<Option<Vec<String>>, SearchError> {
    let Some(query) = query.filter(|query| !query.trim().is_empty()) else {
        return Ok(None);
    };
    if query.len() > MAX_SEARCH_BYTES {
        return Err(SearchError::TooLong);
    }
    // Split before case conversion: Unicode lowercase can introduce combining
    // marks, which must stay attached exactly as they do in the index tokenizer.
    let terms: BTreeSet<_> = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    if terms.len() > MAX_SEARCH_TERMS {
        return Err(SearchError::TooManyTerms);
    }
    Ok(Some(terms.into_iter().collect()))
}

/// Supported item-list views.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemView {
    /// Items discovered in the last day.
    #[default]
    Today,
    /// All saved items, newest first.
    #[serde(alias = "all")]
    Library,
    /// Items with no read state, or an explicit unread state.
    Unread,
    /// Items in progress, newest reading update first.
    Reading,
    /// Starred items, newest star update first.
    Starred,
    /// Items discovered through a particular subscription.
    Feed {
        /// Canonical subscription URL.
        url: String,
    },
}

/// Exclusive boundary in a list ordered by descending time and ascending ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCursor {
    /// The view that produced this cursor; cursors cannot cross filters.
    pub view: ItemView,
    /// Discovery, reading, or star timestamp, depending on the view.
    pub sort_at_micros: i64,
    /// Stable identity breaks timestamp ties.
    pub id: String,
    /// Fixed start of Today's window for subsequent pages.
    pub since_micros: Option<i64>,
    /// Normalized query that produced the cursor; queries cannot share cursors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_terms: Option<Vec<String>>,
}

impl ItemCursor {
    /// Whether another boundary has reached or passed this one.
    ///
    /// Used to refresh an already loaded archive without losing its older end.
    pub fn reached_by(&self, other: &Self) -> bool {
        self.view == other.view
            && self.search_terms == other.search_terms
            && (other.sort_at_micros < self.sort_at_micros
                || (other.sort_at_micros == self.sort_at_micros && other.id >= self.id))
    }
}

/// Item-list query accepted by the native shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    /// List membership and order.
    #[serde(default)]
    pub view: ItemView,
    /// Page size, clamped to `ITEM_PAGE_SIZE` by the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Exclusive boundary returned by the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<ItemCursor>,
    /// Literal title and cached-text search within this view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl Default for ItemFilter {
    fn default() -> Self {
        Self::today()
    }
}

impl ItemFilter {
    /// Recent subscription arrivals.
    pub fn today() -> Self {
        Self::new(ItemView::Today)
    }
    /// Complete local archive.
    pub fn library() -> Self {
        Self::new(ItemView::Library)
    }
    /// Unread archive.
    pub fn unread() -> Self {
        Self::new(ItemView::Unread)
    }
    /// Articles in progress.
    pub fn reading() -> Self {
        Self::new(ItemView::Reading)
    }
    /// Starred archive.
    pub fn starred() -> Self {
        Self::new(ItemView::Starred)
    }
    /// Archive from one feed.
    pub fn feed(url: String) -> Self {
        Self::new(ItemView::Feed { url })
    }
    fn new(view: ItemView) -> Self {
        Self {
            view,
            limit: Some(ITEM_PAGE_SIZE),
            after: None,
            search: None,
        }
    }
}

/// One bounded page of articles plus an optional continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPage<T> {
    /// Articles in this page.
    pub items: Vec<T>,
    /// Another page exists when this boundary is present.
    pub next_cursor: Option<ItemCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_refresh_boundaries_follow_time_then_identity() {
        let boundary = ItemCursor {
            view: ItemView::Library,
            sort_at_micros: 5,
            id: "b".into(),
            since_micros: None,
            search_terms: None,
        };
        let mut candidate = boundary.clone();
        assert!(boundary.reached_by(&candidate));
        candidate.id = "a".into();
        assert!(!boundary.reached_by(&candidate));
        candidate.sort_at_micros = 4;
        assert!(boundary.reached_by(&candidate));
        candidate.sort_at_micros = 6;
        assert!(!boundary.reached_by(&candidate));
        candidate.view = ItemView::Unread;
        assert!(!boundary.reached_by(&candidate));
        candidate = boundary.clone();
        candidate.search_terms = Some(vec!["different".into()]);
        assert!(!boundary.reached_by(&candidate));
    }

    #[test]
    fn search_words_are_literal_case_insensitive_and_unicode_aware() {
        assert_eq!(search_terms(None).unwrap(), None);
        assert_eq!(search_terms(Some(" \t\n")).unwrap(), None);
        assert_eq!(search_terms(Some("\"*()--")).unwrap(), Some(vec![]));
        assert_eq!(
            search_terms(Some("CAFÉ cafe Café \"OR\" rust-lang")).unwrap(),
            Some(vec![
                "cafe".into(),
                "café".into(),
                "lang".into(),
                "or".into(),
                "rust".into()
            ])
        );
        assert_eq!(
            search_terms(Some("İSTANBUL 中文")).unwrap(),
            Some(vec!["i\u{307}stanbul".into(), "中文".into()])
        );
    }

    #[test]
    fn search_queries_are_bounded() {
        assert_eq!(
            search_terms(Some(&"é".repeat(MAX_SEARCH_BYTES))),
            Err(SearchError::TooLong)
        );
        let many = (0..=MAX_SEARCH_TERMS)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(search_terms(Some(&many)), Err(SearchError::TooManyTerms));
    }

    proptest::proptest! {
        #[test]
        fn token_sets_are_order_independent_and_ignore_duplicates(words in proptest::collection::vec("[a-zA-Z0-9]{1,12}", 0..32)) {
            let original = words.join(" ");
            let reversed = words.iter().rev().chain(words.iter()).cloned().collect::<Vec<_>>().join(" / ");
            proptest::prop_assert_eq!(search_terms(Some(&original)), search_terms(Some(&reversed)));
        }
    }
}
