//! Rule-based structural detection for readable article HTML.

mod audio;
mod code;
mod length;
mod link_roundup;
mod math;
mod video;

use carrel_core::shape::Shape;
use kuchiki::traits::TendrilSink;

/// Detect structural shape facts from sanitized readable HTML.
pub fn detect_shape(html: &str, word_count: usize) -> Shape {
    let document = kuchiki::parse_html().one(html);
    Shape {
        has_video_embed: video::detect(&document),
        has_audio_embed: audio::detect(&document),
        is_link_roundup: link_roundup::detect(&document, word_count),
        is_long_form: length::is_long_form(word_count),
        is_short: length::is_short(word_count),
        has_code: code::detect(&document),
        has_math: math::detect(&document),
    }
}

pub use length::{LONG_FORM_WORD_THRESHOLD, SHORT_WORD_THRESHOLD};
