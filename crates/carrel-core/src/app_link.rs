//! Stable private app addresses. These name existing data, never grant access.

use std::{fmt, str::FromStr};

/// A validated saved entity ID, independent of display metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkId(String);

impl LinkId {
    /// Validate the existing 32-byte hex identity and normalize hex case.
    pub fn new(value: &str) -> Result<Self, AppLinkError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppLinkError::InvalidId);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Canonical lowercase identity used by the local store.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LinkId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Fixed reading destinations with no parameters or executable actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppView {
    /// Publications from the preceding day.
    Today,
    /// In-progress reading.
    Reading,
    /// The whole archive.
    Library,
    /// The unread archive.
    Unread,
    /// The starred archive.
    Starred,
    /// Annotation review.
    Highlights,
    /// Articles deliberately shared with this reader.
    Shared,
}

impl AppView {
    /// Internal route for an allow-listed destination.
    pub const fn path(self) -> &'static str {
        match self {
            Self::Today => "/today",
            Self::Reading => "/reading",
            Self::Library => "/library",
            Self::Unread => "/unread",
            Self::Starred => "/starred",
            Self::Highlights => "/highlights",
            Self::Shared => "/friends",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            _ => self.path().trim_start_matches('/'),
        }
    }
}

/// A canonical app link; highlights are resolved to their owning item natively.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppLink {
    /// One existing saved item.
    Item(LinkId),
    /// One existing immutable highlight.
    Highlight(LinkId),
    /// A fixed reading destination.
    View(AppView),
}

impl AppLink {
    /// Address one saved item by its existing identity.
    pub fn item(id: &str) -> Result<Self, AppLinkError> {
        Ok(Self::Item(LinkId::new(id)?))
    }

    /// Address one highlight without binding the link to a note revision.
    pub fn highlight(id: &str) -> Result<Self, AppLinkError> {
        Ok(Self::Highlight(LinkId::new(id)?))
    }
}

impl fmt::Display for AppLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Item(id) => write!(formatter, "carrel://item/{id}"),
            Self::Highlight(id) => write!(formatter, "carrel://highlight/{id}"),
            Self::View(view) => write!(formatter, "carrel://{}", view.name()),
        }
    }
}

/// An external URL is not a supported read-only Carrel address.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AppLinkError {
    /// Only the explicit application scheme is accepted.
    #[error("expected a carrel:// app link")]
    InvalidScheme,
    /// No arbitrary paths, query parameters, fragments, files or actions.
    #[error("unsupported app-link destination")]
    UnknownDestination,
    /// Saved IDs are exactly 32 bytes, represented as hexadecimal.
    #[error("app-link identity must be 64 hexadecimal characters")]
    InvalidId,
    /// Every supported address is short and bounded before parsing.
    #[error("app link exceeds its length limit")]
    TooLong,
}

impl FromStr for AppLink {
    type Err = AppLinkError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > 128 {
            return Err(AppLinkError::TooLong);
        }
        let (scheme, target) = value.split_once("://").ok_or(AppLinkError::InvalidScheme)?;
        if !scheme.eq_ignore_ascii_case("carrel") {
            return Err(AppLinkError::InvalidScheme);
        }
        let (kind, path) = target.split_once('/').unwrap_or((target, ""));
        match kind.to_ascii_lowercase().as_str() {
            "item" => Self::item(path),
            "highlight" => Self::highlight(path),
            name if path.is_empty() => {
                let view = match name {
                    "today" => AppView::Today,
                    "reading" => AppView::Reading,
                    "library" => AppView::Library,
                    "unread" => AppView::Unread,
                    "starred" => AppView::Starred,
                    "highlights" => AppView::Highlights,
                    "shared" => AppView::Shared,
                    _ => return Err(AppLinkError::UnknownDestination),
                };
                Ok(Self::View(view))
            }
            _ => Err(AppLinkError::UnknownDestination),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn every_fixed_destination_has_one_canonical_address_and_route() {
        for (name, view, route) in [
            ("today", AppView::Today, "/today"),
            ("reading", AppView::Reading, "/reading"),
            ("library", AppView::Library, "/library"),
            ("unread", AppView::Unread, "/unread"),
            ("starred", AppView::Starred, "/starred"),
            ("highlights", AppView::Highlights, "/highlights"),
            ("shared", AppView::Shared, "/friends"),
        ] {
            let canonical = format!("carrel://{name}");
            let link = AppLink::View(view);
            assert_eq!(link.to_string(), canonical);
            assert_eq!(canonical.parse::<AppLink>().unwrap(), link);
            assert_eq!(format!("{canonical}/").parse::<AppLink>().unwrap(), link);
            assert_eq!(view.path(), route);
        }
    }

    #[test]
    fn addresses_accept_hex_case_but_reject_parameters_traversal_and_executable_targets() {
        let id = "ab".repeat(32);
        for kind in ["item", "highlight"] {
            let canonical = format!("carrel://{kind}/{id}");
            assert_eq!(
                canonical
                    .to_uppercase()
                    .parse::<AppLink>()
                    .unwrap()
                    .to_string(),
                canonical
            );
            for suffix in ["/", "/../other", "?action=remove", "#passage", "%00", "\n"] {
                assert!(format!("{canonical}{suffix}").parse::<AppLink>().is_err());
            }
        }
        for input in [
            "",
            "carrel:item/abc",
            "https://item/abc",
            "file:///tmp/archive",
            "javascript:alert(1)",
            "carrel-blob://abc",
            "carrel://settings",
            "carrel://remove/abc",
            "carrel://library?query=abc",
            "carrel://library#x",
            "carrel://library/extra",
            "carrel://user@library",
            "carrel://library:80",
            " carrel://library",
            "carrel://library\n",
            "carrel://item/",
            "carrel://highlight/abc",
            "carrel://item/../../etc/passwd",
            "carrel://item/%2F",
            "carrel://LIBRÄRY",
        ] {
            assert!(input.parse::<AppLink>().is_err(), "{input}");
        }
        assert_eq!(
            format!("carrel://item/{}", "g".repeat(64)).parse::<AppLink>(),
            Err(AppLinkError::InvalidId)
        );
        assert_eq!(
            format!("carrel://item/{}", "a".repeat(200)).parse::<AppLink>(),
            Err(AppLinkError::TooLong)
        );
    }

    proptest! {
        #[test]
        fn saved_id_addresses_round_trip_without_metadata(bytes in any::<[u8; 32]>()) {
            let id = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            for link in [AppLink::item(&id).unwrap(), AppLink::highlight(&id).unwrap()] {
                prop_assert_eq!(link.to_string().parse::<AppLink>().unwrap(), link);
            }
        }
    }
}
