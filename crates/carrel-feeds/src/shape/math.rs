//! Math markup and TeX marker detection.

use kuchiki::NodeRef;

const MATH_CLASS_MARKERS: &[&str] = &["mathjax", "katex", "math"];
const TEX_MARKERS: &[&str] = &["\\(", "\\[", "$$", "\\frac", "\\sum", "\\int", "\\alpha"];

pub(crate) fn detect(document: &NodeRef) -> bool {
    has_mathml(document)
        || has_math_class(document)
        || html_contains_tex_markers(document)
        || image_alt_contains_tex(document)
}

fn has_mathml(document: &NodeRef) -> bool {
    document
        .select("math, mrow, mi, mo, mn, msup, msub, mfrac")
        .is_ok_and(|mut matches| matches.next().is_some())
}

fn has_math_class(document: &NodeRef) -> bool {
    let Ok(nodes) = document.select("[class]") else {
        return false;
    };

    nodes.into_iter().any(|node| {
        node.attributes
            .borrow()
            .get("class")
            .is_some_and(|classes| {
                let classes = classes.to_ascii_lowercase();
                MATH_CLASS_MARKERS
                    .iter()
                    .any(|marker| classes.contains(marker))
            })
    })
}

fn html_contains_tex_markers(document: &NodeRef) -> bool {
    let text = document.text_contents();
    TEX_MARKERS.iter().any(|marker| text.contains(marker))
}

fn image_alt_contains_tex(document: &NodeRef) -> bool {
    let Ok(images) = document.select("img[alt]") else {
        return false;
    };

    images.into_iter().any(|image| {
        image
            .attributes
            .borrow()
            .get("alt")
            .is_some_and(|alt| TEX_MARKERS.iter().any(|marker| alt.contains(marker)))
    })
}
