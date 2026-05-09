//! Highlight and annotation selector primitives.

use serde::{Deserialize, Serialize};

/// Maximum prefix/suffix context length, counted in Unicode scalar values.
pub const MAX_TEXT_CONTEXT_CHARS: usize = 256;

/// Text selector against normalized readable HTML text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HtmlTextSelector {
    /// Readable-content blob used when the selector was created, if known.
    pub content_blob_id: Option<String>,
    /// Exact selected text in normalized readable text.
    pub exact: String,
    /// Bounded text before the selection, used for fuzzy relocation.
    pub prefix: String,
    /// Bounded text after the selection, used for fuzzy relocation.
    pub suffix: String,
    /// Start offset in normalized readable text, counted in characters.
    pub start: u32,
    /// End offset in normalized readable text, counted in characters.
    pub end: u32,
}

impl HtmlTextSelector {
    /// Validate selector shape without access to the target content.
    pub fn validate(&self) -> Result<(), SelectorError> {
        if self.exact.is_empty() {
            return Err(SelectorError::EmptyExact);
        }
        if self.start >= self.end {
            return Err(SelectorError::InvalidRange {
                start: self.start,
                end: self.end,
            });
        }
        if self.prefix.chars().count() > MAX_TEXT_CONTEXT_CHARS {
            return Err(SelectorError::ContextTooLong {
                field: "prefix",
                max: MAX_TEXT_CONTEXT_CHARS,
                actual: self.prefix.chars().count(),
            });
        }
        if self.suffix.chars().count() > MAX_TEXT_CONTEXT_CHARS {
            return Err(SelectorError::ContextTooLong {
                field: "suffix",
                max: MAX_TEXT_CONTEXT_CHARS,
                actual: self.suffix.chars().count(),
            });
        }
        Ok(())
    }

    /// Validate selector shape and exact-text match against readable text.
    ///
    /// The content is normalized with [`normalize_readable_text`] before
    /// character offsets are applied. The selector's `exact` text is expected to
    /// already use the same normalization; UI capture should normalize before
    /// submitting.
    pub fn validate_against_text(&self, readable_text: &str) -> Result<(), SelectorError> {
        self.validate()?;
        let normalized = normalize_readable_text(readable_text);
        let Some(selected) = char_range(&normalized, self.start, self.end) else {
            return Err(SelectorError::RangeOutOfBounds {
                end: self.end,
                len: char_len(&normalized),
            });
        };
        if selected == self.exact {
            Ok(())
        } else {
            Err(SelectorError::ExactMismatch {
                expected: self.exact.clone(),
                actual: selected,
            })
        }
    }
}

/// Highlight location, tagged for stable JSON storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HighlightLocation {
    /// A selection inside normalized readable HTML text.
    HtmlText(HtmlTextSelector),
}

impl HighlightLocation {
    /// Validate location shape without access to target content.
    pub fn validate(&self) -> Result<(), SelectorError> {
        match self {
            Self::HtmlText(selector) => selector.validate(),
        }
    }

    /// Return the quoted text represented by this location.
    pub fn quoted_text(&self) -> &str {
        match self {
            Self::HtmlText(selector) => &selector.exact,
        }
    }

    /// Validate against normalized readable text when applicable.
    pub fn validate_against_text(&self, readable_text: &str) -> Result<(), SelectorError> {
        match self {
            Self::HtmlText(selector) => selector.validate_against_text(readable_text),
        }
    }
}

/// Normalize readable text for selector offsets and exact matching.
pub fn normalize_readable_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn char_range(value: &str, start: u32, end: u32) -> Option<String> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    if start >= end {
        return None;
    }
    let chars = value.chars().collect::<Vec<_>>();
    if end > chars.len() {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

fn char_len(value: &str) -> u32 {
    u32::try_from(value.chars().count()).unwrap_or(u32::MAX)
}

/// Selector validation errors.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SelectorError {
    /// The exact quote was empty.
    #[error("selected text cannot be empty")]
    EmptyExact,

    /// The selector range was empty or inverted.
    #[error("invalid text range {start}..{end}")]
    InvalidRange {
        /// Start offset.
        start: u32,
        /// End offset.
        end: u32,
    },

    /// Prefix or suffix context exceeded the bounded length.
    #[error("{field} context is {actual} characters; maximum is {max}")]
    ContextTooLong {
        /// Field name.
        field: &'static str,
        /// Maximum allowed characters.
        max: usize,
        /// Actual characters.
        actual: usize,
    },

    /// The selector range exceeded the available text.
    #[error("range end {end} exceeds text length {len}")]
    RangeOutOfBounds {
        /// Requested end offset.
        end: u32,
        /// Available normalized text length.
        len: u32,
    },

    /// The selected range did not match the quoted text.
    #[error("selector exact text did not match readable text")]
    ExactMismatch {
        /// Exact text from the selector.
        expected: String,
        /// Text found at the selector range.
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_text_location_round_trips_through_tagged_json() {
        let location = HighlightLocation::HtmlText(HtmlTextSelector {
            content_blob_id: Some("blob-1".to_string()),
            exact: "quiet web".to_string(),
            prefix: "the".to_string(),
            suffix: "matters".to_string(),
            start: 4,
            end: 13,
        });

        let value = serde_json::to_value(&location).unwrap();

        assert_eq!(value["type"], "html_text");
        assert_eq!(
            serde_json::from_value::<HighlightLocation>(value.clone()).unwrap(),
            location
        );
        assert_eq!(serde_json::to_value(&location).unwrap(), value);
    }

    #[test]
    fn selector_validation_matches_normalized_text_offsets() {
        let selector = HtmlTextSelector {
            content_blob_id: None,
            exact: "quiet web".to_string(),
            prefix: "the".to_string(),
            suffix: "matters".to_string(),
            start: 4,
            end: 13,
        };

        selector
            .validate_against_text("the \n quiet\tweb   matters")
            .unwrap();
    }

    #[test]
    fn selector_validation_rejects_empty_invalid_and_mismatched_ranges() {
        let mut selector = HtmlTextSelector {
            content_blob_id: None,
            exact: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            start: 0,
            end: 1,
        };
        assert_eq!(selector.validate(), Err(SelectorError::EmptyExact));

        selector.exact = "hello".to_string();
        selector.start = 1;
        selector.end = 1;
        assert_eq!(
            selector.validate(),
            Err(SelectorError::InvalidRange { start: 1, end: 1 })
        );

        selector.start = 0;
        selector.end = 5;
        assert!(matches!(
            selector.validate_against_text("hullo"),
            Err(SelectorError::ExactMismatch { .. })
        ));
    }
}
