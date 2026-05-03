//! Code-heavy article detection.

use kuchiki::NodeRef;

pub(crate) fn detect(document: &NodeRef) -> bool {
    if document
        .select("pre")
        .is_ok_and(|mut matches| matches.next().is_some())
    {
        return true;
    }

    inline_code_count(document) >= 2
}

fn inline_code_count(document: &NodeRef) -> usize {
    let Ok(nodes) = document.select("code") else {
        return 0;
    };

    nodes
        .into_iter()
        .filter(|node| !has_anchor_ancestor(node.as_node()))
        .count()
}

fn has_anchor_ancestor(node: &NodeRef) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor
            .as_element()
            .is_some_and(|element| element.name.local.as_ref() == "a")
    })
}
