//! Word-count based shape detectors.

/// Articles above this word count are long-form.
///
/// 2500 words is long enough to reserve for a deliberate reading session while
/// keeping medium posts in the neutral middle band.
pub const LONG_FORM_WORD_THRESHOLD: usize = 2_500;

/// Articles below this word count are short.
///
/// 300 words covers announcements, short notes, and link posts without
/// capturing ordinary essays.
pub const SHORT_WORD_THRESHOLD: usize = 300;

/// Return true when the article is above the long-form threshold.
pub(crate) fn is_long_form(word_count: usize) -> bool {
    word_count > LONG_FORM_WORD_THRESHOLD
}

/// Return true when the article is below the short threshold.
pub(crate) fn is_short(word_count: usize) -> bool {
    word_count < SHORT_WORD_THRESHOLD
}
