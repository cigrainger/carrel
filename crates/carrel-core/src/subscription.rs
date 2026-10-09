//! Portable subscription metadata, separate from device-local HTTP state.

use serde::{Deserialize, Serialize};

/// A subscription's portable identity and organization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedSubscription {
    /// Feed URL; validated and canonicalized by the store before persistence.
    pub url: String,
    /// Display title, when supplied by the imported outline.
    pub title: Option<String>,
    /// Folder label. Nested OPML folders become a readable ` / ` path.
    pub folder: Option<String>,
}
