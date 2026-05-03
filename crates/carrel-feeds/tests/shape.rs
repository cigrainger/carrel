use carrel_feeds::{detect_shape, sanitize_html};
use proptest::prelude::*;

const NORMAL_WORDS: usize = 900;

#[test]
fn detects_video_embeds() {
    let shape = detect_shape(
        include_str!("shape_fixtures/video_embed_youtube.html"),
        NORMAL_WORDS,
    );

    assert!(shape.has_video_embed);
    assert!(!shape.has_audio_embed);
}

#[test]
fn detects_audio_embeds() {
    let shape = detect_shape(
        include_str!("shape_fixtures/audio_embed_soundcloud.html"),
        NORMAL_WORDS,
    );

    assert!(shape.has_audio_embed);
    assert!(!shape.has_video_embed);
}

#[test]
fn detects_link_roundups() {
    let shape = detect_shape(
        include_str!("shape_fixtures/link_roundup_typical.html"),
        100,
    );

    assert!(shape.is_link_roundup);
}

#[test]
fn detects_length_bands() {
    let long = detect_shape(
        include_str!("shape_fixtures/long_form_3000_words.html"),
        3_001,
    );
    let short = detect_shape(include_str!("shape_fixtures/short_announcement.html"), 120);

    assert!(long.is_long_form);
    assert!(!long.is_short);
    assert!(short.is_short);
    assert!(!short.is_long_form);
}

#[test]
fn detects_code_heavy_articles() {
    let shape = detect_shape(
        include_str!("shape_fixtures/code_heavy_tutorial.html"),
        NORMAL_WORDS,
    );

    assert!(shape.has_code);
}

#[test]
fn detects_math_markup() {
    let shape = detect_shape(
        include_str!("shape_fixtures/math_paper_excerpt.html"),
        NORMAL_WORDS,
    );

    assert!(shape.has_math);
}

#[test]
fn baseline_essay_is_neutral() {
    let shape = detect_shape(
        include_str!("shape_fixtures/baseline_essay.html"),
        NORMAL_WORDS,
    );

    assert!(!shape.has_video_embed);
    assert!(!shape.has_audio_embed);
    assert!(!shape.is_link_roundup);
    assert!(!shape.is_long_form);
    assert!(!shape.is_short);
    assert!(!shape.has_code);
    assert!(!shape.has_math);
}

#[test]
fn false_positive_confusables_stay_neutral() {
    let one_code = detect_shape("<p>Mention <code>foo()</code> once.</p>", NORMAL_WORDS);
    let youtube_link = detect_shape(
        r#"<p>See <a href="https://youtube.com/watch?v=abc">this video</a>.</p>"#,
        NORMAL_WORDS,
    );
    let two_links = detect_shape(
        r#"<p>See <a href="https://a.example">one</a> and <a href="https://b.example">two</a>.</p>"#,
        100,
    );

    assert!(!one_code.has_code);
    assert!(!youtube_link.has_video_embed);
    assert!(!two_links.is_link_roundup);
}

#[test]
fn sanitized_detection_matches_safe_raw_detection() {
    let raw = r#"
        <article>
          <p>Read <a href="https://a.example">one</a> and <a href="https://b.example">two</a>.</p>
          <iframe src="https://www.youtube.com/embed/abc123" title="Video"></iframe>
          <iframe src="https://w.soundcloud.com/player/?url=https%3A//api.soundcloud.com/tracks/123" title="Audio"></iframe>
          <pre><code>let x = 1;</code></pre>
          <span class="katex">x = 1</span>
        </article>
    "#;

    assert_eq!(
        detect_shape(raw, NORMAL_WORDS),
        detect_shape(&sanitize_html(raw), NORMAL_WORDS)
    );
}

proptest! {
    #[test]
    fn detection_is_deterministic(input in ".*", word_count in 0usize..5000) {
        prop_assert_eq!(detect_shape(&input, word_count), detect_shape(&input, word_count));
    }

    #[test]
    fn short_detection_is_monotonic_under_padding(word_count in 0usize..300) {
        let short = detect_shape("<p>short</p>", word_count);
        let padded = detect_shape("<p>short</p>", 300);

        prop_assert!(short.is_short);
        prop_assert!(!padded.is_short);
    }
}
