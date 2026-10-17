//! Bounded OPML subscription interchange, with no filesystem or network I/O.

use std::collections::BTreeMap;

use carrel_core::subscription::FeedSubscription;
use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

/// Maximum import size: large enough for thousands of ordinary subscriptions.
pub const MAX_OPML_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_SUBSCRIPTIONS: usize = 10_000;

/// An OPML document cannot be parsed or represented as subscriptions.
#[derive(Debug, thiserror::Error)]
#[error("invalid OPML: {0}")]
pub struct OpmlError(String);

#[derive(Default)]
struct Element {
    name: String,
    folder: Option<String>,
}

/// Parse all subscriptions before any caller writes to its store.
///
/// UTF-8 OPML 1.x and 2.0 are accepted. DTDs, external entities, unsupported
/// encodings, malformed XML, and invalid feed URLs are rejected. Nested folders
/// are retained as readable paths in Carrel's existing single folder field.
pub fn parse_opml(bytes: &[u8]) -> Result<Vec<FeedSubscription>, OpmlError> {
    if bytes.len() > MAX_OPML_BYTES {
        return fail("document exceeds 4 MiB");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| OpmlError("use a UTF-8 file".into()))?;
    if text.chars().any(|character| !valid_xml_char(character)) {
        return fail("document contains an invalid XML character");
    }
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    let mut stack = Vec::<Element>::new();
    let mut subscriptions = Vec::new();
    let mut root_seen = false;
    let mut body_seen = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| OpmlError(error.to_string()))?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                let name = element.name().as_ref().to_string();
                let mut attributes = BTreeMap::new();
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| OpmlError(error.to_string()))?;
                    attributes.insert(
                        attribute.key.as_ref().to_string(),
                        attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|error| OpmlError(error.to_string()))?
                            .into_owned(),
                    );
                }
                if attributes
                    .values()
                    .any(|value| value.chars().any(|character| !valid_xml_char(character)))
                {
                    return fail("attribute contains an invalid XML character");
                }
                if stack.len() >= MAX_DEPTH {
                    return fail("outline nesting is too deep");
                }
                if stack.is_empty() {
                    if root_seen || name != "opml" {
                        return fail("expected one opml root element");
                    }
                    root_seen = true;
                    if attributes
                        .get("version")
                        .is_some_and(|version| !matches!(version.as_str(), "1.0" | "1.1" | "2.0"))
                    {
                        return fail("unsupported OPML version");
                    }
                }
                let mut folder = None;
                if name == "body" {
                    if stack.len() != 1 || stack[0].name != "opml" || body_seen {
                        return fail("expected one body directly inside opml");
                    }
                    body_seen = true;
                }
                if name == "outline" {
                    if !stack.iter().any(|element| element.name == "body")
                        || !matches!(
                            stack.last().map(|element| element.name.as_str()),
                            Some("body" | "outline")
                        )
                    {
                        return fail("outline must be inside the body");
                    }
                    let title = attributes
                        .get("title")
                        .or_else(|| attributes.get("text"))
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    if let Some(url) = attributes.get("xmlUrl") {
                        let url = url.trim();
                        validate_url(url)?;
                        if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                            return fail("too many subscriptions");
                        }
                        let folders = stack
                            .iter()
                            .filter_map(|element| element.folder.as_deref())
                            .collect::<Vec<_>>();
                        subscriptions.push(FeedSubscription {
                            url: url.into(),
                            title,
                            folder: (!folders.is_empty()).then(|| folders.join(" / ")),
                        });
                    } else {
                        folder = title;
                    }
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(Element { name, folder });
                }
            }
            Event::End(element) => {
                let Some(parent) = stack.pop() else {
                    return fail("unbalanced elements");
                };
                if parent.name != element.name().as_ref() {
                    return fail("unbalanced elements");
                }
            }
            Event::Decl(declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|error| OpmlError(error.to_string()))?;
                    if !encoding.eq_ignore_ascii_case("utf-8") {
                        return fail("use a UTF-8 file");
                    }
                }
            }
            Event::DocType(_) => return fail("DOCTYPE declarations are unsupported"),
            Event::GeneralRef(reference) => {
                if stack.is_empty() {
                    return fail("reference outside opml");
                }
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|error| OpmlError(error.to_string()))?
                {
                    if !valid_xml_char(character) {
                        return fail("invalid character reference");
                    }
                } else if !matches!(reference.as_ref(), "amp" | "lt" | "gt" | "quot" | "apos") {
                    return fail("unknown entity reference");
                }
            }
            Event::Text(text) if stack.is_empty() && !text.as_ref().trim().is_empty() => {
                return fail("text outside opml");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !body_seen || !stack.is_empty() {
        return fail("missing or incomplete opml body");
    }
    Ok(subscriptions)
}

/// Write portable UTF-8 OPML, grouped by folder label and sorted by URL.
///
/// Carrel's folder labels remain intact. An imported nested path is exported
/// as a single named folder; no HTTP cache state or personal reading data leaves
/// the store in a subscription export.
pub fn export_opml(subscriptions: &[FeedSubscription]) -> Result<Vec<u8>, OpmlError> {
    let mut groups = BTreeMap::<Option<&str>, Vec<&FeedSubscription>>::new();
    for subscription in subscriptions {
        validate_url(&subscription.url)?;
        for value in [
            subscription.title.as_deref(),
            subscription.folder.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.chars().any(|character| !valid_xml_char(character)) {
                return fail("title or folder contains an invalid XML character");
            }
        }
        groups
            .entry(subscription.folder.as_deref())
            .or_default()
            .push(subscription);
    }
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut write = |event| {
        writer
            .write_event(event)
            .map_err(|error| OpmlError(error.to_string()))
    };
    write(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    let mut root = BytesStart::new("opml");
    root.push_attribute(("version", "2.0"));
    write(Event::Start(root))?;
    write(Event::Start(BytesStart::new("head")))?;
    write(Event::Start(BytesStart::new("title")))?;
    write(Event::Text(BytesText::new("Carrel subscriptions")))?;
    write(Event::End(BytesEnd::new("title")))?;
    write(Event::End(BytesEnd::new("head")))?;
    write(Event::Start(BytesStart::new("body")))?;
    for (folder, mut feeds) in groups {
        if let Some(folder) = folder {
            let mut outline = BytesStart::new("outline");
            outline.push_attribute(("text", folder));
            write(Event::Start(outline))?;
        }
        feeds.sort_by(|a, b| a.url.cmp(&b.url));
        for feed in feeds {
            let mut outline = BytesStart::new("outline");
            outline.push_attribute(("type", "rss"));
            outline.push_attribute(("text", feed.title.as_deref().unwrap_or(&feed.url)));
            outline.push_attribute(("title", feed.title.as_deref().unwrap_or("")));
            outline.push_attribute(("xmlUrl", feed.url.as_str()));
            write(Event::Empty(outline))?;
        }
        if folder.is_some() {
            write(Event::End(BytesEnd::new("outline")))?;
        }
    }
    write(Event::End(BytesEnd::new("body")))?;
    write(Event::End(BytesEnd::new("opml")))?;
    Ok(writer.into_inner())
}

fn validate_url(value: &str) -> Result<(), OpmlError> {
    let url = url::Url::parse(value).map_err(|_| OpmlError("feed URL is invalid".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return fail("feed URL must use http or https without credentials");
    }
    Ok(())
}
fn valid_xml_char(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
fn fail<T>(message: &str) -> Result<T, OpmlError> {
    Err(OpmlError(message.into()))
}
