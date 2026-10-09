//! Shared library query contracts, independent of storage and presentation.

use serde::{Deserialize, Serialize};

/// The number of articles requested in one archive page.
pub const ITEM_PAGE_SIZE: usize = 100;

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
}

impl ItemCursor {
    /// Whether another boundary has reached or passed this one.
    ///
    /// Used to refresh an already loaded archive without losing its older end.
    pub fn reached_by(&self, other: &Self) -> bool {
        self.view == other.view
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
    }
}
