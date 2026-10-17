//! Complete, readable annotation exports through a native save dialog.

use std::{collections::BTreeMap, fmt::Write as _, io::Write as _, path::Path};

use carrel_core::app_link::AppLink;
use carrel_store::{Store, highlights::AnnotationReview};
use tauri::{State, Window};
use tauri_plugin_dialog::DialogExt;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{AppError, Result, state::AppState};

/// Export the complete living archive, independently of the loaded review page.
#[tauri::command]
pub async fn export_annotations(window: Window, state: State<'_, AppState>) -> Result<bool> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Export highlights and notes")
        .set_file_name("carrel-annotations.md")
        .add_filter("Markdown", &["md"])
        .save_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver.await.map_err(file_error)? else {
        return Ok(false);
    };
    let path = file.into_path().map_err(file_error)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || export_to_path(&store, &path))
        .await
        .map_err(file_error)??;
    Ok(true)
}

fn export_to_path(store: &Store, path: &Path) -> Result<()> {
    let document = markdown(&store.annotation_export_snapshot()?);
    let parent = path
        .parent()
        .ok_or_else(|| file_error("destination has no parent folder"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(file_error)?;
    file.write_all(document.as_bytes()).map_err(file_error)?;
    file.as_file().sync_all().map_err(file_error)?;
    file.persist(path).map_err(file_error)?;
    Ok(())
}

fn markdown(rows: &[AnnotationReview]) -> String {
    let mut output = "# Carrel — highlights and notes\n\n".to_string();
    if rows.is_empty() {
        output.push_str("No highlights saved.\n");
    }
    let mut articles: BTreeMap<&str, Vec<&AnnotationReview>> = BTreeMap::new();
    for row in rows {
        articles
            .entry(&row.source.highlight.item_id)
            .or_default()
            .push(row);
    }
    for highlights in articles.values() {
        let article = &highlights[0].source;
        let _ = writeln!(
            output,
            "## {}\n",
            literal(&article.item_title).replace('\n', " ")
        );
        if let Some(url) = article.item_primary_url.as_deref().and_then(safe_url) {
            let _ = writeln!(output, "[Open article](<{url}>)\n");
        }
        if let Ok(link) = AppLink::item(&article.highlight.item_id) {
            let _ = writeln!(output, "[Open in Carrel](<{link}>)\n");
        }
        for row in highlights {
            let highlight = &row.source.highlight;
            let _ = writeln!(
                output,
                "### Highlight · {}\n",
                date(highlight.created_at_micros)
            );
            for line in highlight.quoted_text.lines() {
                let _ = writeln!(output, "> {}", literal(line));
            }
            let _ = writeln!(output, "\nReference: `{}`\n", highlight.id);
            if let Ok(link) = AppLink::highlight(&highlight.id) {
                let _ = writeln!(output, "[Open highlight in Carrel](<{link}>)\n");
            }
            for note in &row.notes {
                if let Some(versions) = row.conflicts.get(&note.id) {
                    let _ = writeln!(
                        output,
                        "#### Note · unresolved versions\n\nReference: `{}`\n",
                        note.id
                    );
                    for version in versions {
                        let _ = writeln!(
                            output,
                            "##### Unresolved version · {}\n\n{}\n\nRevision: `{}`\n",
                            date(version.updated_at_micros),
                            version.body,
                            version.revision_id
                        );
                    }
                } else {
                    let _ = writeln!(
                        output,
                        "#### Note · {}\n\n{}\n\nReference: `{}`\n",
                        date(note.updated_at_micros),
                        note.body,
                        note.id
                    );
                }
            }
            output.push_str("---\n\n");
        }
    }
    output
}

fn safe_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return None;
    }
    Some(url.as_str().replace('<', "%3C").replace('>', "%3E"))
}

fn literal(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        if "\\`*_{}[]()#+-.!|>".contains(character) {
            escaped.push('\\');
        }
        match character {
            '<' => escaped.push_str("&lt;"),
            '&' => escaped.push_str("&amp;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn date(micros: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros) * 1_000)
        .ok()
        .and_then(|date| date.format(&Rfc3339).ok())
        .unwrap_or_else(|| micros.to_string())
}

fn file_error(error: impl std::fmt::Display) -> AppError {
    AppError::AnnotationFile(format!("Annotation file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::{
        annotation::{HighlightLocation, HtmlTextSelector},
        identity::Keypair,
    };
    use carrel_store::highlights::{EditNoteInput, PutHighlightInput, PutNoteInput};

    #[test]
    fn markdown_preserves_full_latest_notes_quotes_references_and_atomic_files() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.query(r#"?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
            [['1212121212121212121212121212121212121212121212121212121212121212', 'article', 'A [quiet] <web>', [], 'https://example.com/page?q=<test>', null, null, null, validity(1)]]
            :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}"#).unwrap();
        let key = Keypair::generate();
        let highlight = store
            .put_highlight(
                PutHighlightInput {
                    item_id: "12".repeat(32),
                    location: HighlightLocation::HtmlText(HtmlTextSelector {
                        content_blob_id: None,
                        exact: "*quoted* 日本語\n<script>".into(),
                        prefix: String::new(),
                        suffix: String::new(),
                        start: 0,
                        end: 30,
                    }),
                    location_label: None,
                    color: None,
                    created_at_micros: Some(1),
                },
                &key,
            )
            .unwrap();
        let note = store
            .put_note(
                PutNoteInput {
                    body: "old note".into(),
                    target_kind: Some("highlight".into()),
                    target_id: Some(highlight.id.clone()),
                    created_at_micros: Some(2),
                },
                &key,
            )
            .unwrap();
        let body = format!("**My note**\n\n{}\n\nLast paragraph.", "café ".repeat(100));
        store
            .edit_note(
                EditNoteInput {
                    id: note.id.clone(),
                    body: body.clone(),
                    expected_revision_id: note.revision_id.clone(),
                    now_micros: Some(3),
                },
                &key,
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("annotations.md");
        std::fs::write(&path, "old file").unwrap();
        export_to_path(&store, &path).unwrap();
        let document = std::fs::read_to_string(&path).unwrap();
        assert!(document.contains(&body));
        assert!(!document.contains("old note"));
        assert!(document.contains("\\*quoted\\* 日本語\n> &lt;script\\>"));
        assert!(document.contains(&highlight.id) && document.contains(&note.id));
        assert!(document.contains(&format!(
            "[Open in Carrel](<carrel://item/{}>)",
            "12".repeat(32)
        )));
        assert!(document.contains(&format!(
            "[Open highlight in Carrel](<carrel://highlight/{}>)",
            highlight.id
        )));
        assert!(document.contains("A \\[quiet\\] &lt;web\\>"));
        assert!(document.contains("[Open article](<https://example.com/page?q=%3Ctest%3E>)"));
        assert!(export_to_path(&store, &dir.path().join("missing/file.md")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), document);
        store.remove_highlight(&highlight.id, &key).unwrap();
        export_to_path(&store, &path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains(&note.id));
    }

    #[test]
    fn empty_exports_and_unsafe_links_are_readable_and_inert() {
        assert!(markdown(&[]).contains("No highlights saved."));
        for raw in [
            "javascript:alert(1)",
            "file:///private/a",
            "data:text/html,a",
            "/relative",
            "invalid",
        ] {
            assert_eq!(safe_url(raw), None);
        }
        assert_eq!(
            safe_url("https://example.com/a b"),
            Some("https://example.com/a%20b".into())
        );
    }

    #[test]
    fn unresolved_equal_time_versions_are_complete_in_review_and_exports() {
        use carrel_core::identity::DeviceCert;
        use carrel_store::notes::ResolveNoteInput;

        let a = Store::open_in_memory().unwrap();
        let b = Store::open_in_memory().unwrap();
        let ka = Keypair::from_seed(&[71; 32]);
        let kb = Keypair::from_seed(&[72; 32]);
        let master = Keypair::from_seed(&[73; 32]);
        let input = PutHighlightInput {
            item_id: "item".into(),
            location: HighlightLocation::HtmlText(HtmlTextSelector {
                exact: "A quiet passage".into(),
                content_blob_id: None,
                prefix: String::new(),
                suffix: String::new(),
                start: 0,
                end: 15,
            }),
            location_label: None,
            color: None,
            created_at_micros: Some(1),
        };
        for store in [&a, &b] {
            store.migrate().unwrap();
            store.query("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [['item', 'article', 'Article', [], null, null, null, null, validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}").unwrap();
            store.put_highlight(input.clone(), &ka).unwrap();
        }
        let ca = DeviceCert::new_at_micros(&master, ka.public(), 1, Some("Desktop".into()));
        let cb = DeviceCert::new_at_micros(&master, kb.public(), 1, Some("Laptop".into()));
        a.bind_self_identity(&ca, &ka).unwrap();
        b.bind_self_identity(&cb, &kb).unwrap();
        a.accept_self_device_certificate(&cb).unwrap();
        b.accept_self_device_certificate(&ca).unwrap();
        let highlight = a.list_item_highlights("item").unwrap().remove(0);
        let root = a
            .put_note(
                PutNoteInput {
                    body: "Original".into(),
                    target_kind: Some("highlight".into()),
                    target_id: Some(highlight.id.clone()),
                    created_at_micros: Some(1),
                },
                &ka,
            )
            .unwrap();
        b.accept_self_note_revision(&root, ka.public()).unwrap();
        let mut versions = Vec::new();
        for (store, key, body) in [
            (&a, &ka, format!("Desktop draft\n\n{}", "café ".repeat(200))),
            (
                &b,
                &kb,
                format!("Laptop draft\n\n{}", "日本語 ".repeat(200)),
            ),
        ] {
            versions.push(
                store
                    .edit_note(
                        EditNoteInput {
                            id: root.id.clone(),
                            body,
                            expected_revision_id: root.revision_id.clone(),
                            now_micros: Some(2),
                        },
                        key,
                    )
                    .unwrap(),
            );
        }
        a.accept_self_note_revision(&versions[1], kb.public())
            .unwrap();
        let snapshot = a.annotation_export_snapshot().unwrap();
        assert_eq!(snapshot[0].notes.len(), 1);
        assert_eq!(snapshot[0].conflicts[&root.id].len(), 2);
        let document = markdown(&snapshot);
        for version in &versions {
            assert!(document.contains(&version.body));
            assert!(document.contains(&version.revision_id));
        }
        assert!(document.contains("unresolved versions"));
        assert!(!document.contains("Original"));
        let row = crate::commands::highlights::list_highlights_from_store(
            &a,
            None,
            None,
            Some(ka.public()),
        )
        .unwrap()
        .items
        .remove(0);
        assert_eq!(
            crate::commands::highlights::review_note_from_store(&a, &root.id, Some(ka.public()))
                .unwrap(),
            row.notes[0]
        );
        assert!(
            !crate::commands::highlights::review_note_from_store(
                &a,
                &root.id,
                Some(Keypair::from_seed(&[74; 32]).public())
            )
            .unwrap()
            .can_edit
        );
        assert!(row.notes[0].can_edit);
        assert_eq!(row.notes[0].conflicts.len(), 2);
        for (label, version) in ["Desktop", "Laptop"].into_iter().zip(&versions) {
            let dto = row.notes[0]
                .conflicts
                .iter()
                .find(|dto| dto.revision_id == version.revision_id)
                .unwrap();
            assert_eq!(dto.body, version.body);
            assert_eq!(dto.device_label, label);
            assert!(!dto.saved_at.is_empty());
        }
        a.resolve_note(
            ResolveNoteInput {
                id: root.id.clone(),
                body: "Deliberately combined".into(),
                expected_revision_ids: a
                    .note_heads(&root.id)
                    .unwrap()
                    .into_iter()
                    .map(|record| record.revision_id)
                    .collect(),
                now_micros: Some(3),
            },
            &ka,
        )
        .unwrap();
        let document = markdown(&a.annotation_export_snapshot().unwrap());
        assert!(
            crate::commands::highlights::review_note_from_store(&a, &root.id, Some(ka.public()))
                .unwrap()
                .conflicts
                .is_empty()
        );
        assert!(document.contains("Deliberately combined"));
        assert!(!document.contains("unresolved versions"));
        assert!(!document.contains("Desktop draft") && !document.contains("Laptop draft"));
    }
}
