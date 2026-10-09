//! Reader highlight selection capture and DOM reattachment helpers.

use carrel_core::annotation::{
    HighlightLocation, HtmlTextSelector, MAX_TEXT_CONTEXT_CHARS, normalize_readable_text,
};

use crate::api::{CreateHighlightRequest, HighlightDetail};

/// Build a highlight request from reader text and a captured browser selection.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn request_from_selection_text(
    item_id: String,
    content_blob_id: Option<String>,
    body_text: &str,
    before_selection_text: &str,
    selected_text: &str,
) -> Option<CreateHighlightRequest> {
    let selector = selector_from_text_parts(
        content_blob_id,
        body_text,
        before_selection_text,
        selected_text,
    )?;

    Some(CreateHighlightRequest {
        item_id,
        location: HighlightLocation::HtmlText(selector),
        location_label: None,
        color: Some("yellow".to_string()),
    })
}

/// Build a text selector from raw body text plus range text.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn selector_from_text_parts(
    content_blob_id: Option<String>,
    body_text: &str,
    before_selection_text: &str,
    selected_text: &str,
) -> Option<HtmlTextSelector> {
    let exact = normalize_readable_text(selected_text);
    if exact.is_empty() {
        return None;
    }

    let normalized_body = normalize_readable_text(body_text);
    let leading_space = selected_text.find(|char: char| !char.is_whitespace())?;
    let start = normalized_prefix_len(&format!(
        "{before_selection_text}{}",
        &selected_text[..leading_space]
    ));
    let end = start.checked_add(exact.chars().count())?;
    if normalized_slice(&normalized_body, start, end).as_deref() != Some(exact.as_str()) {
        return None;
    }

    Some(HtmlTextSelector {
        content_blob_id,
        exact,
        prefix: context_before(&normalized_body, start, MAX_TEXT_CONTEXT_CHARS),
        suffix: context_after(&normalized_body, end, MAX_TEXT_CONTEXT_CHARS),
        start: u32::try_from(start).ok()?,
        end: u32::try_from(end).ok()?,
    })
}

/// Collapse whitespace like `normalize_readable_text`, but preserve a trailing
/// separator because selector starts are offsets in the full normalized body.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn normalized_prefix_len(value: &str) -> usize {
    let mut len = 0_usize;
    let mut emitted_any = false;
    let mut pending_space = false;

    for char in value.chars() {
        if char.is_whitespace() {
            if emitted_any {
                pending_space = true;
            }
            continue;
        }

        if pending_space {
            len += 1;
            pending_space = false;
        }
        len += 1;
        emitted_any = true;
    }

    if pending_space {
        len += 1;
    }

    len
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn context_before(value: &str, end: usize, max: usize) -> String {
    let start = end.saturating_sub(max);
    char_slice(value, start, end).unwrap_or_default()
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn context_after(value: &str, start: usize, max: usize) -> String {
    let end = start.saturating_add(max).min(value.chars().count());
    char_slice(value, start, end).unwrap_or_default()
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn normalized_slice(value: &str, start: usize, end: usize) -> Option<String> {
    if start >= end {
        return None;
    }
    char_slice(value, start, end)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn char_slice(value: &str, start: usize, end: usize) -> Option<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if start > end || end > chars.len() {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct NormalizedChar {
    raw_start: usize,
    raw_end: usize,
}

/// Map normalized readable-text offsets back to raw `text_content` offsets.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct NormalizedTextMap {
    normalized: String,
    chars: Vec<NormalizedChar>,
}

impl NormalizedTextMap {
    /// Build a normalized text map from raw DOM text.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn new(raw_text: &str) -> Self {
        let mut normalized = String::new();
        let mut chars = Vec::new();
        let mut emitted_any = false;
        let mut pending_space_start = None;
        let mut pending_space_end = 0_usize;

        for (raw_index, char) in raw_text.chars().enumerate() {
            if char.is_whitespace() {
                if emitted_any {
                    pending_space_start.get_or_insert(raw_index);
                    pending_space_end = raw_index + 1;
                }
                continue;
            }

            if let Some(raw_start) = pending_space_start.take() {
                normalized.push(' ');
                chars.push(NormalizedChar {
                    raw_start,
                    raw_end: pending_space_end,
                });
            }

            normalized.push(char);
            chars.push(NormalizedChar {
                raw_start: raw_index,
                raw_end: raw_index + 1,
            });
            emitted_any = true;
        }

        Self { normalized, chars }
    }

    /// Return normalized text.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn normalized(&self) -> &str {
        &self.normalized
    }

    /// Map a normalized char range to raw char offsets.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn raw_range(&self, start: u32, end: u32) -> Option<(usize, usize)> {
        let start = usize::try_from(start).ok()?;
        let end = usize::try_from(end).ok()?;
        if start >= end || end > self.chars.len() {
            return None;
        }

        Some((self.chars[start].raw_start, self.chars[end - 1].raw_end))
    }
}

/// A highlight range selected for DOM rendering.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct RenderRange {
    /// Highlight id.
    pub(crate) id: String,
    /// Normalized start offset.
    pub(crate) start: u32,
    /// Normalized end offset.
    pub(crate) end: u32,
}

/// Return deterministic non-overlapping ranges to render.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn visible_render_ranges(
    highlights: &[HighlightDetail],
    normalized_text: &str,
) -> Vec<RenderRange> {
    let mut ranges = highlights
        .iter()
        .filter_map(|highlight| render_range(highlight, normalized_text))
        .collect::<Vec<_>>();

    ranges.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.end.cmp(&left.end))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut visible = Vec::new();
    let mut occupied_until = 0_u32;
    for range in ranges {
        if range.start < occupied_until {
            continue;
        }
        occupied_until = range.end;
        visible.push(range);
    }
    visible
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn render_range(highlight: &HighlightDetail, normalized_text: &str) -> Option<RenderRange> {
    let HighlightLocation::HtmlText(selector) = &highlight.location;
    selector.validate_against_text(normalized_text).ok()?;
    Some(RenderRange {
        id: highlight.id.clone(),
        start: selector.start,
        end: selector.end,
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn capture_current_selection(
    body: &web_sys::HtmlElement,
    item_id: String,
    content_blob_id: Option<String>,
) -> Option<CreateHighlightRequest> {
    let window = web_sys::window()?;
    let selection = window.get_selection().ok().flatten()?;
    if selection.is_collapsed() || selection.range_count() == 0 {
        return None;
    }

    let range = selection.get_range_at(0).ok()?;
    let body_node = body.clone().into();
    if !node_contains(&body_node, &range.start_container().ok()?)
        || !node_contains(&body_node, &range.end_container().ok()?)
    {
        return None;
    }

    let document = body.owner_document()?;
    let before_range = document.create_range().ok()?;
    before_range.select_node_contents(&body_node).ok()?;
    before_range
        .set_end(&range.start_container().ok()?, range.start_offset().ok()?)
        .ok()?;

    let before_text = String::from(before_range.to_string());
    let selected_text = String::from(selection.to_string());

    request_from_selection_text(
        item_id,
        content_blob_id,
        &body.text_content().unwrap_or_default(),
        &before_text,
        &selected_text,
    )
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_current_selection() {
    if let Some(selection) =
        web_sys::window().and_then(|window| window.get_selection().ok().flatten())
    {
        let _ = selection.remove_all_ranges();
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn render_highlights(
    body: &web_sys::HtmlElement,
    content_html: &str,
    highlights: &[HighlightDetail],
    focus_highlight_id: Option<&str>,
) -> Vec<String> {
    body.set_inner_html(content_html);

    let raw_text = body.text_content().unwrap_or_default();
    let text_map = NormalizedTextMap::new(&raw_text);
    let ranges = visible_render_ranges(highlights, text_map.normalized());
    let mut failed = Vec::new();

    for range in ranges.iter().rev() {
        let Some((raw_start, raw_end)) = text_map.raw_range(range.start, range.end) else {
            failed.push(range.id.clone());
            continue;
        };
        if wrap_raw_range(body, raw_start, raw_end, &range.id).is_err() {
            failed.push(range.id.clone());
        }
    }

    if let Some(id) = focus_highlight_id
        && !focus_highlight(body, id)
    {
        failed.push(id.to_string());
    }

    for id in &failed {
        web_sys::console::warn_1(&format!("Could not reattach highlight {id}").into());
    }

    failed
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn current_hash_highlight_id() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    hash.strip_prefix("#highlight-")
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
}

#[cfg(target_arch = "wasm32")]
fn wrap_raw_range(
    body: &web_sys::HtmlElement,
    raw_start: usize,
    raw_end: usize,
    id: &str,
) -> Result<(), wasm_bindgen::JsValue> {
    if raw_start >= raw_end {
        return Ok(());
    }

    let document = body.owner_document().ok_or_else(|| {
        wasm_bindgen::JsValue::from_str("reader body did not have an owner document")
    })?;
    let mut nodes = Vec::new();
    collect_text_nodes(&body.clone().into(), &mut nodes);
    let lengths = nodes
        .iter()
        .map(|node| node.node_value().unwrap_or_default().chars().count())
        .collect::<Vec<_>>();
    let segments = text_segments(&lengths, raw_start, raw_end)
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("missing highlight boundaries"))?;
    // Wrap text within each existing element: extracting a cross-paragraph range
    // splits paragraphs and can move links, lists, or code into an invalid span.
    for (index, start, end) in segments.into_iter().rev() {
        let node = &nodes[index];
        let text = node.node_value().unwrap_or_default();
        let range = document.create_range()?;
        range.set_start(
            node,
            utf16_offset_for_char(&text, start)
                .ok_or_else(|| wasm_bindgen::JsValue::from_str("highlight offset overflow"))?,
        )?;
        range.set_end(
            node,
            utf16_offset_for_char(&text, end)
                .ok_or_else(|| wasm_bindgen::JsValue::from_str("highlight offset overflow"))?,
        )?;
        let span = document.create_element("span")?;
        span.set_class_name("carrel-highlight");
        span.set_attribute("data-carrel-highlight-id", id)?;
        span.set_attribute("tabindex", "-1")?;
        let fragment = range.extract_contents()?;
        span.append_child(&fragment)?;
        range.insert_node(&span)?;
    }
    Ok(())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn text_segments(
    lengths: &[usize],
    start: usize,
    end: usize,
) -> Option<Vec<(usize, usize, usize)>> {
    if start >= end || end > lengths.iter().sum() {
        return None;
    }
    let mut offset = 0;
    let mut segments = Vec::new();
    for (index, &length) in lengths.iter().enumerate() {
        let local_start = start.saturating_sub(offset);
        let local_end = end.saturating_sub(offset).min(length);
        if local_start < local_end {
            segments.push((index, local_start, local_end));
        }
        offset += length;
    }
    Some(segments)
}

#[cfg(target_arch = "wasm32")]
fn collect_text_nodes(root: &web_sys::Node, out: &mut Vec<web_sys::Node>) {
    if root.node_type() == web_sys::Node::TEXT_NODE {
        out.push(root.clone());
        return;
    }

    let children = root.child_nodes();
    for index in 0..children.length() {
        if let Some(child) = children.item(index) {
            collect_text_nodes(&child, out);
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn utf16_offset_for_char(value: &str, char_offset: usize) -> Option<u32> {
    let offset = value
        .chars()
        .take(char_offset)
        .map(char::len_utf16)
        .sum::<usize>();
    u32::try_from(offset).ok()
}

#[cfg(target_arch = "wasm32")]
fn focus_highlight(body: &web_sys::HtmlElement, id: &str) -> bool {
    let selector = format!(r#"[data-carrel-highlight-id="{id}"]"#);
    let Ok(Some(element)) = body.query_selector(&selector) else {
        return false;
    };

    element.scroll_into_view();
    if let Some(html) = wasm_bindgen::JsCast::dyn_ref::<web_sys::HtmlElement>(&element) {
        let _ = html.focus();
    }
    true
}

#[cfg(target_arch = "wasm32")]
fn node_contains(root: &web_sys::Node, node: &web_sys::Node) -> bool {
    let mut current = Some(node.clone());
    while let Some(candidate) = current {
        if candidate.is_same_node(Some(root)) {
            return true;
        }
        current = candidate.parent_node();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(id: &str, start: u32, end: u32, exact: &str) -> HighlightDetail {
        HighlightDetail {
            id: id.to_string(),
            item_id: "item-1".to_string(),
            quoted_text: exact.to_string(),
            location: HighlightLocation::HtmlText(HtmlTextSelector {
                content_blob_id: Some("blob-1".to_string()),
                exact: exact.to_string(),
                prefix: String::new(),
                suffix: String::new(),
                start,
                end,
            }),
            location_label: None,
            color: Some("yellow".to_string()),
            created_by: "0".repeat(64),
            created_at: "2026-05-09T00:00:00Z".to_string(),
            created_at_micros: 1,
            signature: "0".repeat(128),
        }
    }

    #[test]
    fn selector_capture_preserves_separator_offsets() {
        let selector = selector_from_text_parts(
            Some("blob-1".to_string()),
            "The quiet   web matters.",
            "The ",
            "quiet web",
        )
        .unwrap();

        assert_eq!(selector.start, 4);
        assert_eq!(selector.end, 13);
        assert_eq!(selector.prefix, "The ");
        assert_eq!(selector.suffix, " matters.");
    }

    #[test]
    fn selections_starting_with_whitespace_keep_the_correct_offset() {
        for (body, before, selection, expected) in [
            ("The quiet web", "The", " quiet", 4),
            ("The  quiet web", "The ", " quiet ", 4),
            ("The\nquiet web", "The", "\nquiet", 4),
            ("  quiet web", "", "  quiet", 0),
            ("éclair\u{2003}静かな", "éclair", "\u{2003}静かな", 7),
        ] {
            let selector = selector_from_text_parts(None, body, before, selection).unwrap();
            assert_eq!(selector.start, expected);
            assert_eq!(selector.exact, selection.trim());
        }
    }

    #[test]
    fn selector_capture_rejects_empty_or_mismatched_selection() {
        assert!(selector_from_text_parts(None, "The quiet web", "The ", " ").is_none());
        assert!(selector_from_text_parts(None, "The quiet web", "The ", "different").is_none());
    }

    #[test]
    fn reattachment_keeps_paragraphs_and_inline_elements_in_place() {
        // Three text nodes in separate elements, with an empty node between.
        assert_eq!(
            text_segments(&[3, 0, 7, 5], 3, 12),
            Some(vec![(2, 0, 7), (3, 0, 2)])
        );
        assert_eq!(
            text_segments(&[3, 7, 5], 2, 10),
            Some(vec![(0, 2, 3), (1, 0, 7)])
        );
        assert_eq!(text_segments(&[3], 0, 4), None);
        assert_eq!(text_segments(&[], 0, 0), None);
        assert_eq!(utf16_offset_for_char("a🙂é", 2), Some(3));
    }

    #[test]
    fn normalized_text_map_maps_collapsed_spaces_to_raw_ranges() {
        let map = NormalizedTextMap::new("  The quiet \n web   matters. ");

        assert_eq!(map.normalized(), "The quiet web matters.");
        assert_eq!(map.raw_range(4, 13), Some((6, 17)));
    }

    #[test]
    fn visible_ranges_skip_overlaps_but_keep_adjacent_highlights() {
        let rows = vec![
            highlight("shorter-same-start", 4, 9, "quiet"),
            highlight("longest-same-start", 4, 13, "quiet web"),
            highlight("overlaps", 10, 21, "web matters"),
            highlight("adjacent", 14, 22, "matters."),
        ];

        let ranges = visible_render_ranges(&rows, "The quiet web matters.");

        assert_eq!(
            ranges,
            vec![
                RenderRange {
                    id: "longest-same-start".to_string(),
                    start: 4,
                    end: 13,
                },
                RenderRange {
                    id: "adjacent".to_string(),
                    start: 14,
                    end: 22,
                },
            ]
        );
    }
}
