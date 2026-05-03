//! Structural content-shape facts shared by feeds, store, and CLI layers.

/// Rule-based structural facts detected from readable item content.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Shape {
    /// The readable body contains an embedded video or known video player.
    pub has_video_embed: bool,
    /// The readable body contains embedded audio or a known audio player.
    pub has_audio_embed: bool,
    /// The readable body has high outbound-link density.
    pub is_link_roundup: bool,
    /// The readable body is longer than the long-form threshold.
    pub is_long_form: bool,
    /// The readable body is shorter than the short-form threshold.
    pub is_short: bool,
    /// The readable body contains code blocks or multiple inline code spans.
    pub has_code: bool,
    /// The readable body contains MathML, TeX markers, or known math classes.
    pub has_math: bool,
}
