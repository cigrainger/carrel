//! Bounded presentation preferences for the local reader.

use serde::{Deserialize, Serialize};

/// Bundled reading faces, independent of the application chrome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingFont {
    /// Default screen-reading serif.
    #[default]
    SourceSerif,
    /// Book-oriented serif.
    Literata,
    /// Classic Garalde serif.
    Garamond,
    /// Sans-serif reading option.
    Inter,
}

/// Article list density, independent of reading typography.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListDensity {
    /// Full editorial rows with excerpts.
    #[default]
    Editorial,
    /// Compact rows with wrapping titles.
    Dense,
}

/// Installation-local reading preferences. No reading facts are changed by these.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReadingPreferences {
    /// Bundled body face.
    pub font: ReadingFont,
    /// Body size in CSS pixels.
    pub size: u8,
    /// Maximum measure in body-font characters.
    pub measure: u8,
    /// Line height as a percentage of text size.
    pub leading: u8,
    /// Density of article lists.
    pub density: ListDensity,
}

impl Default for ReadingPreferences {
    fn default() -> Self {
        Self {
            font: ReadingFont::default(),
            size: 18,
            measure: 68,
            leading: 165,
            density: ListDensity::default(),
        }
    }
}

impl ReadingPreferences {
    /// Bound stored or supplied values to the supported reading range.
    pub fn normalized(mut self) -> Self {
        self.size = self.size.clamp(14, 26);
        self.measure = self.measure.clamp(52, 82);
        self.leading = self.leading.clamp(145, 185);
        self
    }

    /// Restore typography while retaining the independent list preference.
    pub fn reset_reading(self) -> Self {
        Self {
            density: self.density,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn old_preferences_keep_defaults_for_new_fields_and_invalid_fonts_are_rejected() {
        let preferences: ReadingPreferences = serde_json::from_str(r#"{"size":21}"#).unwrap();
        assert_eq!(preferences.size, 21);
        assert_eq!(preferences.measure, 68);
        assert_eq!(preferences.font, ReadingFont::SourceSerif);
        assert!(serde_json::from_str::<ReadingPreferences>(r#"{"font":"unknown"}"#).is_err());
        assert!(serde_json::from_str::<ReadingPreferences>(r#"{"size":-1}"#).is_err());
        for font in [
            ReadingFont::SourceSerif,
            ReadingFont::Literata,
            ReadingFont::Garamond,
            ReadingFont::Inter,
        ] {
            for density in [ListDensity::Editorial, ListDensity::Dense] {
                let p = ReadingPreferences {
                    font,
                    density,
                    ..ReadingPreferences::default()
                };
                assert_eq!(
                    serde_json::from_str::<ReadingPreferences>(&serde_json::to_string(&p).unwrap())
                        .unwrap(),
                    p
                );
                assert_eq!(p.reset_reading().density, density);
            }
        }
    }

    proptest! {
        #[test]
        fn normalization_is_bounded_and_idempotent(size in any::<u8>(), measure in any::<u8>(), leading in any::<u8>()) {
            let p = ReadingPreferences { size, measure, leading, ..ReadingPreferences::default() }.normalized();
            prop_assert!((14..=26).contains(&p.size));
            prop_assert!((52..=82).contains(&p.measure));
            prop_assert!((145..=185).contains(&p.leading));
            prop_assert_eq!(p.normalized(), p);
        }
    }
}
